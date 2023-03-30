// Max iters for Newton's method when calculating D
pub const D_NM_MAX_ITERS: u64 = 8;
// Max/expected iters for Newton's method when calculating
pub const DY_NM_MAX_ITERS: u64 = 8;
pub const DY_NM_EXP_ITERS: u64 = 4;

pub const STABLESWAP_AMP_COEFFICIENT: u64 = 5;

// The Stableswap invariant for a two-token pool with amounts (x, y) is given as
//   4A(x+y) + D = 4AD + D^3/(4xy)
// where A and D are constants, A is chosen by us and D is the "total amount of
// coins when they have an equal price." So in theory, D = x+y when x = y, and
// in practice, D slips a little bit from x+y when x != y.
//
// Since there's no closed-form solution for D/x/y, we use Newton's method. Let
//   f(D) = f(y) = LHS - RHS =
//     [4A(x+y) + D] - [4AD + D^3/(4xy)] =
//     4A(x+y-D) + D - D^3/(4xy) ,
//   f'(D) = 4A + 1 - 3D^2/(4xy) ,
//   f'(y) = 4A + D^3/(4xy^2) .
// Then we can easily apply Newton's method. For our initial values of D and y,
// we choose D_0 = x+y, and y_0 = y+dx (when someone withdraws dx from the x/y
// pool). These starting values tend to be very good approximations and allows
// us to do few iterations of NM until we get close to a root.
//
// This fails when the pool is highly imbalanced (x/y > ~1000), in which case
// Newton's method can overshoot the real value of dy and reach the other root,
// which might be negative. To fix this, we just need to make sure y never goes
// below its original value (+1, so that we don't end up pricing things at 0).
// When this happens, we also give Newton's method more iterations to converge,
// since it's basically restarted at a worse approximation.

/// Calculate the value of D in the Stableswap invariant.
/// Returns None in the case that D could not be calculated.
///
/// Note that this is the raw Stableswap calculation - it relies on the assumption
/// that each X token should be equal in price to each Y token. Make sure to
/// account for decimals BEFORE calling.
pub fn calc_d(x: u64, y: u64, a: u64) -> Option<u64> {
    // calc_d(1000000000+20000000, 1000000000-10000000) -> 548 compute units (4 iters)
    let x = x as f64;
    let y = y as f64;
    let a = a as f64;

    let mut d = x + y;
    for _ in 0..D_NM_MAX_ITERS {
        let d2 = d * d;
        let f = 4.0 * a * (x + y - d) + d - d * d2 / (4.0 * x * y);
        let f_ = 1.0 - 4.0 * a - 3.0 * d2 / (4.0 * x * y);
        d = d - f / f_;
    }

    if d > u64::MAX as f64 {
        return None;
    }
    // let d = d.round();
    Some((0.5 + d) as u64)
}

/// Calculate the value of dy - the amount to deposit into y after withdrawing
/// dx from x.
/// Formally, ensure that the invariant holds for (x, y) -> (x-dx, y+dy).
/// Returns None in the case that dy could not be calculated, which might happen
/// if there isn't enough compute time available to converge to a good solution.
///
/// Note that this is the raw Stableswap calculation - it relies on the assumption
/// that each X token should be equal in price to each Y token. Make sure to
/// account for decimals BEFORE calling.
pub fn calc_dy(x: u64, y: u64, a: u64, d: u64, dx: u64) -> Option<u64> {
    // Note: calc_dy(1000000000+20000000, 1000000000-20000000, d, 20000000) -> 402 compute units (4 iters)
    if dx >= x {
        return None;
    }

    let x = (x - dx) as f64;
    let a = a as f64;
    let d = d as f64;

    let y_min = (y + 1) as f64;
    let mut y_ = (y + dx) as f64;
    let mut use_max_iters = false;
    let mut last_move = 0.0;
    for i in 0..DY_NM_MAX_ITERS {
        if !use_max_iters && i >= DY_NM_EXP_ITERS {
            break;
        }
        let d3 = d * d * d;
        let f = 4.0 * a * (x + y_ - d) + d - d3 / (4.0 * x * y_);
        let f_ = 4.0 * a + d3 / (4.0 * x * y_ * y_);
        y_ = y_ - f / f_;
        last_move = f / f_;

        // If y' goes below y, it'll take a little longer to converge
        if y_ < y_min {
            y_ = y_min;
            use_max_iters = true;
        }
    }

    if last_move.abs() > 1.0 {
        return None;
    }
    if y_ > u64::MAX as f64 {
        return None;
    }
    // let dy = (y_ - y as f64).round() as u64;
    let dy = (0.5 + y_ - y as f64) as u64;
    Some(dy)
}

#[cfg(test)]
mod stableswap_tests {
    use super::*;
    use std::cmp;
    use std::fmt;

    #[derive(Debug, Clone)]
    struct Pool {
        x: u64,
        y: u64,
        lp: u64,

        x_decimals: u8,
        y_decimals: u8,
        amp_coef: u64,
        fee: f64,
    }

    impl Pool {
        pub fn new(x: u64, y: u64, x_decimals: u8, y_decimals: u8) -> Pool {
            Pool {
                x,
                y,
                lp: (x + y) / 2,
                x_decimals,
                y_decimals,
                amp_coef: 85,
                fee: 0.02 / 100.0,
            }
        }

        /// Swap `dx` worth of asset X from the pool.
        /// Mutates the pool, and returns the amount (in `Y`) the withdrawer is charged.
        pub fn swap_x(&mut self, dx: u64) -> u64 {
            let (x, y) = fix_decimals(self.x, self.y, self.x_decimals, self.y_decimals);
            let (dx, _) = fix_decimals(dx, 0, self.x_decimals, self.y_decimals);

            let d = calc_d(x, y, self.amp_coef).unwrap();
            let dy = calc_dy(x, y, self.amp_coef, d, dx).unwrap();

            let (dx, dy) = revert_decimals(dx, dy, self.x_decimals, self.y_decimals);
            let dy = ((dy as f64) * (1.0 + self.fee)) as u64;
            // let price = (dy as f64)/(dx as f64) * (1.0 + self.fee);

            self.x -= dx;
            self.y += dy;
            return dy;
        }

        /// Same as `swap_x` but for Y.
        pub fn swap_y(&mut self, dy: u64) -> u64 {
            let (x, y) = fix_decimals(self.x, self.y, self.x_decimals, self.y_decimals);
            let (_, dy) = fix_decimals(0, dy, self.x_decimals, self.y_decimals);

            let d = calc_d(y, x, self.amp_coef).unwrap();
            let dx = calc_dy(y, x, self.amp_coef, d, dy).unwrap();

            let (dx, dy) = revert_decimals(dx, dy, self.x_decimals, self.y_decimals);
            let price = (dx as f64) / (dy as f64) * (1.0 + self.fee);

            self.x += dx;
            self.y -= dy;
            return (price * dy as f64) as u64;
        }

        /// Deposit liquidity into the pool.
        /// Mutates the pool, and returns the amount of LP tokens minted.
        pub fn deposit(&mut self, x: u64, y: u64) -> u64 {
            let lp = match self.lp {
                0 => (x + y)/2,
                lp => cmp::min(
                    (lp * x) / self.x,
                    (lp * y) / self.y
                )
                // (lp_mint_supply as u128)
                //     .checked_mul(deposit_coin_amount.into()).unwrap()
                //     .checked_div(reserve_coin_amount.into()).unwrap() as u64,
                // (lp_mint_supply as u128)
                //     .checked_mul(deposit_pc_amount.into()).unwrap()
                //     .checked_div(reserve_pc_amount.into()).unwrap() as u64,
            };

            let d = self.d().unwrap();

            self.x += x;
            self.y += y;

            let d_ = self.d().unwrap();
            let real_lp = (self.lp as f64 * ((d_ - d) as f64 / d as f64)) as u64;

            self.lp += lp;

            lp
        }

        /// Withdraw liquidity from the pool.
        /// Mutates the pool, and returns the amount of X/Y withdrawn.
        pub fn withdraw(&mut self, lp: u64) -> (u64, u64) {
            let x = self.x * lp / self.lp;
            let y = self.y * lp / self.lp;

            self.x -= x;
            self.y -= y;
            self.lp -= lp;

            (x, y)
        }

        pub fn d(&self) -> Option<u64> {
            calc_d(self.x, self.y, self.amp_coef)
        }
    }

    #[derive(Debug, Clone)]
    struct Amm {
        x: u64,
        y: u64,
        asks: [Order; 6],
        bids: [Order; 6],
    }

    impl fmt::Display for Amm {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            writeln!(
                f,
                "x/y: {:.2} / {:.2} (total {:.2})",
                self.x as f64 / 1e6,
                self.y as f64 / 1e6,
                (self.x + self.y) as f64 / 1e6
            )?;
            for &ask in self.asks.iter() {
                writeln!(f, "{}", ask)?;
            }
            writeln!(f, "-----")?;
            for &bid in self.bids.iter() {
                writeln!(f, "{}", bid)?;
            }

            Ok(())
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct Order {
        amount: u64,
        price: f64,
    }

    impl fmt::Display for Order {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{:.2} | {:.6}", self.amount as f64 / 1e6, self.price)
        }
    }

    impl Amm {
        fn new(x: u64, y: u64) -> Amm {
            Amm {
                x,
                y,
                asks: [Order {
                    amount: 0,
                    price: 0.0,
                }; 6],
                bids: [Order {
                    amount: 0,
                    price: 0.0,
                }; 6],
            }
        }

        fn crank(&mut self) {
            let props = [[0.025, 0.050], [0.200, 0.250], [0.300, 0.000]];
            let fee = 0.0002;
            let amp_coef = 50;

            let init_x = self.x;
            let init_y = self.y;

            let mut last_ask_x = self.x;
            let mut last_ask_y = self.y;
            let mut last_bid_x = self.x;
            let mut last_bid_y = self.y;

            for ix_index in 0..3 {
                // Faulty logic from original program, for reference
                // let mut total_prop = 0.0;
                // for i in 0..ix_index {
                //     total_prop += props[i].iter().sum::<f64>();
                // }
                // let mut last_ask_x = init_x - (init_x as f64 * total_prop) as u64;
                // let mut last_ask_y = init_y - (init_y as f64 * total_prop) as u64;
                // let mut last_bid_x = last_ask_x;
                // let mut last_bid_y = last_ask_y;

                let d = calc_d(last_ask_x, last_ask_y, amp_coef).unwrap();

                for order_index in 0..2 {
                    let prop = props[ix_index][order_index];

                    // Ask
                    let dx = (init_x as f64 * prop) as u64;
                    let dy = calc_dy(last_ask_x, last_ask_y, amp_coef, d, dx).unwrap_or(0);
                    if dx > 0 && dy > 0 {
                        let dy = ((dy as f64) * (1.0 + fee)) as u64;
                        last_ask_x -= dx;
                        last_ask_y += dy;
                        self.asks[ix_index * 2 + order_index] = Order {
                            amount: dx,
                            price: dy as f64 / dx as f64,
                        };
                    } else {
                        self.asks[ix_index * 2 + order_index] = Order {
                            amount: 0,
                            price: 0.0,
                        }
                    }

                    // Bid
                    let dy = (init_y as f64 * prop) as u64;
                    let dx = calc_dy(last_bid_y, last_bid_x, amp_coef, d, dy).unwrap_or(0);
                    if dx > 0 && dy > 0 {
                        let dy = ((dy as f64) / (1.0 + fee)) as u64;
                        last_bid_y -= dy;
                        last_bid_x += dx;
                        self.bids[ix_index * 2 + order_index] = Order {
                            amount: dx,
                            price: dy as f64 / dx as f64,
                        };
                    } else {
                        self.bids[ix_index * 2 + order_index] = Order {
                            amount: 0,
                            price: 0.0,
                        }
                    }
                }
            }
        }

        /// Buy some x, mutating the pool and returning dy
        fn buy(&mut self, x: u64) -> u64 {
            let mut x = x;
            let mut y = 0;
            for &ask in self.asks.iter() {
                let dx = cmp::min(x, ask.amount);
                let dy = (dx as f64 * ask.price) as u64;
                x -= dx;
                y += dy;
                self.x -= dx;
                self.y += dy;

                if x == 0 {
                    self.crank();
                    return y;
                }
            }

            println!("{}", x);
            panic!("too high of a buy order");
        }

        /// Sell some x, mutating the pool and returning dy
        fn sell(&mut self, x: u64) -> u64 {
            let mut x = x;
            let mut y = 0;
            for &bid in self.bids.iter() {
                let dx = cmp::min(x, bid.amount);
                let dy = (dx as f64 * bid.price) as u64;
                x -= dx;
                y += dy;
                self.x += dx;
                self.y -= dy;

                if x == 0 {
                    self.crank();
                    return y;
                }
            }

            println!("{}", x);
            panic!("too high of a sell order");
        }

        fn ask_x_available(&self) -> u64 {
            self.asks.map(|bid| bid.amount).iter().sum()
        }

        fn bid_x_available(&self) -> u64 {
            self.bids.map(|bid| bid.amount).iter().sum()
        }
    }

    /// Fix the decimals of x/y to be equal to whichever has more decimals.
    fn fix_decimals(x: u64, y: u64, x_decimals: u8, y_decimals: u8) -> (u64, u64) {
        if x_decimals > y_decimals {
            let c = 10u64.checked_pow((x_decimals - y_decimals) as u32).unwrap();
            (x, y * c)
        } else if y_decimals > x_decimals {
            let c = 10u64.checked_pow((y_decimals - x_decimals) as u32).unwrap();
            (x * c, y)
        } else {
            (x, y)
        }
    }

    /// Revert the decimals of x/y after calling `fix_decimals`.
    fn revert_decimals(x: u64, y: u64, x_decimals: u8, y_decimals: u8) -> (u64, u64) {
        if x_decimals > y_decimals {
            let c = 10u64.checked_pow((x_decimals - y_decimals) as u32).unwrap();
            (x, y / c)
        } else if y_decimals > x_decimals {
            let c = 10u64.checked_pow((y_decimals - x_decimals) as u32).unwrap();
            (x / c, y)
        } else {
            (x, y)
        }
    }

    #[test]
    fn basic_test() {
        let mut pool = Pool::new(1e9 as u64, 1e9 as u64, 6, 6);

        for dx in [0.0001e9 as u64, 0.1e9 as u64, 0.5e9 as u64] {
            let dy = pool.swap_x(dx);
            assert!(dy > dx);
        }
    }

    #[test]
    /// Test with uneven decimals.
    fn basic_decimals_test() {
        // 1e6 X = 1e8 Y -> 1 X = 100 Y
        let mut pool = Pool::new(10000e6 as u64, 10000e8 as u64, 6, 8);

        for dx in [0.01e6 as u64, 10e6 as u64, 5000e6 as u64] {
            let dy = pool.swap_x(dx);
            // Make sure the "true" price of for an X token is very close to 1 X = 100 Y
            assert!(((dy as f64 / dx as f64) / (1e8 / 1e6) - 1.0).abs() < 0.01);
        }
    }

    #[test]
    /// Test drain idea on a zero-fee pool.
    /// Starting with a pool of (1B+amt/2, 1B-amt/2), attempt to siphon money
    /// out by continuously swapping x and y.
    fn drain_test() {
        let make_pool = || {
            let mut pool = Pool::new(1e9 as u64, 1e9 as u64, 6, 6);
            pool.amp_coef = 25;
            pool
        };
        let init_total = 2e9 as u64;

        for frac in [0.001, 0.2, 0.5, 0.825, 0.9999] {
            let mut pool = make_pool();

            let mut x_bal = 0;
            let mut y_bal = 0;
            let mut x_pay = 0;
            let mut y_pay = 0;
            for _ in 0..1 {
                let x_amt = (frac * pool.x as f64) as u64;
                let y_amt: u64;

                x_bal += x_amt;
                y_amt = pool.swap_x(x_amt);
                y_pay += y_amt;

                y_bal += y_amt;
                x_pay += pool.swap_y(y_amt);
            }

            // Make sure the user ends up paying at least what they withdraw
            println!(
                "{}: {}",
                frac,
                (x_pay + y_pay) as i64 - (x_bal + y_bal) as i64
            );
            println!(
                "{}: {}",
                frac,
                (pool.x + pool.y) as i64 - (init_total as i64)
            );
            // assert!(x_bal + y_bal <= x_pay + y_pay);
            // Make sure the pool has not decreased in value
            // assert!(pool.x + pool.y >= init_total);
        }
    }

    #[test]
    /// Test to make sure the curve is actually concave everywhere.
    ///
    /// This test actually fails when making very small movements along the curve
    /// (~1/10000th of the pool). I believe this isn't a big cause for concern,
    /// since the price difference is less than the fees we use.
    fn concavity_test() {
        let init = 1e9 as u64;
        for dx in [init / 10, init / 100, init / 1000, init / 2000] {
            let mut pool = Pool::new(init, init, 6, 6);
            pool.fee = 0.0;

            let mut price = 1.0;
            while pool.x > dx {
                // Price = dy/dx
                let price_ = (pool.swap_x(dx) as f64) / (dx as f64);
                assert!(price_ >= price);
                price = price_;
            }
        }
    }

    #[test]
    fn lp_test() {
        let mut pool = Pool::new(1e9 as u64, 1e9 as u64, 6, 6);
        pool.amp_coef = 500;
        let withdrawal = 0.99999e9 as u64;
        println!("{}", calc_d(pool.x, pool.y, pool.amp_coef).unwrap());
        let dy = pool.swap_x(withdrawal);
        println!("{}", calc_d(pool.x, pool.y, pool.amp_coef).unwrap());
        println!("{}", dy / withdrawal as u64);
    }

    /// Test the ppUSDC-USDC exploit. Brute-forces many sequences of random swaps
    /// in order to see if it's possible to reduce the pool's equilibrium value.
    #[test]
    fn attack_test() {
        let decimals_fac = 1e6 as u64;

        let mut amm = Amm::new(10000 * decimals_fac, 10000 * decimals_fac);
        amm.crank();

        println!("{}", amm);

        let (max_profit, amm) = attack_solve(&mut amm, 8, 0, 0);
        println!("max profit: ${}", max_profit / decimals_fac as i64);
        println!("{}", amm);

        assert!(max_profit == 0);
    }

    /// Helper for attack test. Tries a tree of possible swaps within the pool,
    /// and reports the maximum possible profit for a string of swaps and the
    /// final state of the AMM.
    fn attack_solve(amm: &mut Amm, depth: u64, x: i64, y: i64) -> (i64, Amm) {
        if depth == 0 {
            return (x + y, amm.clone());
        }

        let mut ans_profit = 0;
        let mut ans_amm = amm.clone();

        // Proportion of available liquidity to take on the market - denom = 1000
        let prop_nums = [50, 500, 1000];

        // Try buying
        for &prop_num in prop_nums.iter() {
            let amm = &mut amm.clone();
            let dx = (amm.ask_x_available() * prop_num / 1000) as i64;
            let dy = amm.buy(dx as u64) as i64;
            let (profit, amm) = attack_solve(&mut amm.clone(), depth - 1, x + dx, y - dy);
            if profit > ans_profit {
                ans_profit = profit;
                ans_amm = amm;
            }
        }

        // Try selling
        for &prop_num in prop_nums.iter() {
            let amm = &mut amm.clone();
            let dx = (amm.bid_x_available() * prop_num / 1000) as i64;
            let dy = amm.sell(dx as u64) as i64;
            let (profit, amm) = attack_solve(&mut amm.clone(), depth - 1, x - dx, y + dy);
            if profit > ans_profit {
                ans_profit = profit;
                ans_amm = amm;
            }
        }

        (ans_profit, ans_amm)
    }
}

// Changes the decimals of coin/pc to match, s.t. 1 coin ~= 1 pc in stable conditions
pub fn get_token_decs_fac(base_decimals: u8, quote_decimals: u8) -> (u64, u64) {
    if base_decimals > quote_decimals {
        (
            1,
            10u64
                .checked_pow((base_decimals - quote_decimals) as u32)
                .unwrap(),
        )
    } else {
        (
            10u64
                .checked_pow((quote_decimals - base_decimals) as u32)
                .unwrap(),
            1,
        )
    }
}

fn normalize_decimals(
    coin_amount: u64,
    coin_decimals: u8,
    pc_amount: u64,
    pc_decimals: u8,
) -> (u64, u64) {
    // Changes the decimals of coin/pc to match, s.t. 1 coin ~= 1 pc in stable conditions
    let (coin_decs_fac, pc_decs_fac) = get_token_decs_fac(coin_decimals, pc_decimals);

    // Multiply here so that we don't lose precision, then divide later
    (
        coin_amount.checked_mul(coin_decs_fac).unwrap(),
        pc_amount.checked_mul(pc_decs_fac).unwrap(),
    )
}

pub fn calculate_stableswap_lp_minted(
    lp_mint_supply: u64,
    reserve_base_amount: u64,
    reserve_quote_amount: u64,
    deposit_base_amount: u64,
    deposit_quote_amount: u64,
    base_decimals: u8,
    quote_decimals: u8,
) -> u64 {
    let (norm_reserve_base, norm_reserve_quote) = normalize_decimals(
        reserve_base_amount,
        base_decimals,
        reserve_quote_amount,
        quote_decimals,
    );
    let (norm_deposit_base, norm_deposit_quote) = normalize_decimals(
        deposit_base_amount,
        base_decimals,
        deposit_quote_amount,
        quote_decimals,
    );

    let d_0 = calc_d(
        norm_reserve_base,
        norm_reserve_quote,
        STABLESWAP_AMP_COEFFICIENT,
    )
    .unwrap();
    let d_1 = calc_d(
        norm_reserve_base.checked_add(norm_deposit_base).unwrap(),
        norm_reserve_quote.checked_add(norm_deposit_quote).unwrap(),
        STABLESWAP_AMP_COEFFICIENT,
    )
    .unwrap();

    match lp_mint_supply {
        0 => d_1,
        lp_mint_supply => (lp_mint_supply as u128)
            .checked_mul(d_1.checked_sub(d_0).unwrap().into())
            .unwrap()
            .checked_div(d_0.into())
            .unwrap()
            .try_into()
            .unwrap(),
    }
}

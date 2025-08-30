use anyhow::Result;
use itertools::Itertools;

use crate::resolver::Resolver;

pub struct Arbitrage;

impl Arbitrage {
    pub fn new() -> Self {
        Self
    }

    /// Scanare triunghiulară minimală: doar generează rutele și pune profit estimat = 0.0.
    /// O facem să compileze și să putem avansa; ulterior conectăm quotes reale din Resolver.
    pub async fn check_all_routes(
        &self,
        _resolver: &Resolver,
        tokens: &Vec<&'static str>,
        amount: u64,
        slippage_bps: u64,
    ) -> Result<Vec<(String, f64)>> {
        let mut out = Vec::new();

        // itertools::Itertools::permutations(3) -> Iterator<Item = Vec<&str>>
        for perm in tokens.iter().copied().permutations(3) {
            let a = perm[0];
            let b = perm[1];
            let c = perm[2];

            // TODO: aici vom chema resolver-ul pentru 3 quote-uri (A->B, B->C, C->A)
            // și calculăm profitul real. Temporar punem 0.0 ca să treacă build-ul.
            let est_profit = 0.0_f64;

            out.push((
                format!("{} -> {} -> {} (amt={}, slip={}bps)", a, b, c, amount, slippage_bps),
                est_profit,
            ));
        }

        Ok(out)
    }
}
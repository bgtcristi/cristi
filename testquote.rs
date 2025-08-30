use anyhow::Result;
use reqwest::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let url = "https://quote-api.jup.ag/v6/quote?inputMint=So11111111111111111111111111111111111111112&outputMint=Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB&amount=1000000&slippageBps=50";

    let resp = Client::new()
        .get(url)
        .send()
        .await?
        .text()
        .await?;

    println!("RAW response:\n{}", resp);
    Ok(())
}
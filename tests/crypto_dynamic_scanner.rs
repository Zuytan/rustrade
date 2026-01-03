/// Integration test for crypto dynamic scanner
/// This test verifies that:
/// 1. MarketScanner can fetch crypto top movers
/// 2. Top movers are sent to Sentinel
/// 3. Crypto symbols are properly formatted (BTC/USD style)
///
/// Run with: cargo test --test crypto_dynamic_scanner -- --nocapture
///
/// Requirements:
/// - ALPACA_API_KEY and ALPACA_SECRET_KEY must be set in environment
/// - ASSET_CLASS=crypto
/// - Real Alpaca paper trading account

use rustrade::application::agents::scanner::MarketScanner;
use rustrade::application::agents::sentinel::SentinelCommand;
use rustrade::config::AssetClass;
use rustrade::domain::ports::MarketDataService; // Added for trait method access
use rustrade::infrastructure::alpaca::{AlpacaExecutionService, AlpacaMarketDataService};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;

#[tokio::test]
#[ignore] // Requires real API credentials
async fn test_crypto_scanner_integration() {
    // Load credentials from environment
    let api_key = std::env::var("ALPACA_API_KEY").expect("ALPACA_API_KEY not set");
    let secret_key = std::env::var("ALPACA_SECRET_KEY").expect("ALPACA_SECRET_KEY not set");
    let base_url = "https://paper-api.alpaca.markets".to_string();
    let data_url = "https://data.alpaca.markets".to_string();
    let ws_url = "wss://stream.data.alpaca.markets/v2/crypto".to_string();

    // Create services
    let market_service = Arc::new(AlpacaMarketDataService::new(
        api_key.clone(),
        secret_key.clone(),
        ws_url,
        data_url,
        50000.0, // min volume threshold
        AssetClass::Crypto,
    ));

    let execution_service = Arc::new(AlpacaExecutionService::new(
        api_key,
        secret_key,
        base_url,
    ));

    // Create command channel
    let (cmd_tx, mut cmd_rx) = mpsc::channel(10);

    // Create scanner
    let scanner = MarketScanner::new(
        market_service as Arc<dyn rustrade::domain::ports::MarketDataService>,
        execution_service as Arc<dyn rustrade::domain::ports::ExecutionService>,
        cmd_tx,
        Duration::from_secs(10), // Short interval for testing
        true, // enabled
    );

    // Start scanner in background
    tokio::spawn(async move {
        scanner.run().await;
    });

    // Wait for first update (should come within 10 seconds)
    println!("Waiting for MarketScanner to send crypto top movers...");
    let update = tokio::time::timeout(Duration::from_secs(15), cmd_rx.recv())
        .await
        .expect("Timeout waiting for scanner update")
        .expect("Channel closed unexpectedly");

    // Verify update
    match update {
        SentinelCommand::UpdateSymbols(symbols) => {
            println!("Received symbols: {:?}", symbols);
            
            // Verify we got crypto symbols
            assert!(!symbols.is_empty(), "Should have at least one crypto symbol");
            
            // Verify symbols are in correct format (contain '/')
            for sym in &symbols {
                if sym.contains("USD") {
                    assert!(
                        sym.contains('/'),
                        "Crypto symbol {} should be in slash format (e.g., BTC/USD)",
                        sym
                    );
                }
            }
            
            println!("✓ Crypto scanner test passed!");
            println!("✓ Found {} top movers", symbols.len());
        }
        _ => panic!("Expected UpdateSymbols command"),
    }
}

#[tokio::test]
async fn test_crypto_movers_api_call() {
    // This test just verifies the API can be called
    // Skip if no credentials
    if std::env::var("ALPACA_API_KEY").is_err() {
        println!("Skipping test - no API credentials");
        return;
    }

    let api_key = std::env::var("ALPACA_API_KEY").unwrap();
    let secret_key = std::env::var("ALPACA_SECRET_KEY").unwrap();
    let data_url = "https://data.alpaca.markets".to_string();
    let ws_url = "wss://stream.data.alpaca.markets/v2/crypto".to_string();

    let service = AlpacaMarketDataService::new(
        api_key,
        secret_key,
        ws_url,
        data_url,
        50000.0,
        AssetClass::Crypto,
    );

    // Try to get top movers
    match service.get_top_movers().await {
        Ok(movers) => {
            println!("Crypto top movers: {:?}", movers);
            println!("Found {} movers", movers.len());
            
            // Verify we got some movers
            assert!(movers.len() <= 5, "Should return at most 5 movers");
            
            // Verify format
            for mover in &movers {
                if mover.contains("USD") {
                    assert!(
                        mover.contains('/'),
                        "Crypto mover {} should contain '/'",
                        mover
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to fetch crypto movers: {}", e);
            panic!("API call failed: {}", e);
        }
    }
}

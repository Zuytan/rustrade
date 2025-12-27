#!/usr/bin/env python3
"""
Quick test script to verify Alpaca crypto WebSocket connection
"""
import asyncio
import json
import websockets
import os

async def test_alpaca_crypto():
    # Get credentials from environment or use placeholders
    api_key = os.getenv("ALPACA_API_KEY", "YOUR_KEY_HERE")
    api_secret = os.getenv("ALPACA_SECRET_KEY", "YOUR_SECRET_HERE")
    
    # Crypto WebSocket endpoint
    url = "wss://stream.data.alpaca.markets/v1beta3/crypto/us"
    
    print(f"Connecting to {url}...")
    
    async with websockets.connect(url) as websocket:
        print("✅ Connected!")
        
        # Expect welcome message
        welcome = await websocket.recv()
        print(f"📨 Received: {welcome}")
        
        # Send auth
        auth_msg = {
            "action": "auth",
            "key": api_key,
            "secret": api_secret
        }
        await websocket.send(json.dumps(auth_msg))
        print("🔐 Auth sent")
        
        # Wait for auth response
        auth_response = await websocket.recv()
        print(f"📨 Auth response: {auth_response}")
        
        # Subscribe to crypto
        subscribe_msg = {
            "action": "subscribe",
            "trades": ["BTC/USD", "ETH/USD"],
            "quotes": ["BTC/USD", "ETH/USD"]
        }
        await websocket.send(json.dumps(subscribe_msg))
        print("📡 Subscription sent for BTC/USD, ETH/USD")
        
        # Listen for 30 seconds
        print("\n⏳ Listening for 30 seconds...\n")
        try:
            for i in range(30):
                message = await asyncio.wait_for(websocket.recv(), timeout=1.0)
                print(f"[{i+1}s] 📊 {message[:200]}...")  # Print first 200 chars
        except asyncio.TimeoutError:
            print("⏱️  No data received in the last second")
            pass
        
        print("\n✅ Test complete!")

if __name__ == "__main__":
    asyncio.run(test_alpaca_crypto())

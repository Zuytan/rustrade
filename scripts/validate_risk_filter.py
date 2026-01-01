#!/usr/bin/env python3
"""
Comprehensive Benchmark Validation Script
Compares risk-based strategy vs buy-and-hold across multiple periods and symbols
"""

import subprocess
import re
import csv
from datetime import datetime
from pathlib import Path

# Configuration
RESULTS_DIR = Path("benchmark_results")
RESULTS_DIR.mkdir(exist_ok=True)
RESULTS_FILE = RESULTS_DIR / f"risk_filter_validation_{datetime.now().strftime('%Y%m%d_%H%M%S')}.csv"

# Test configuration
PERIODS = [
    ("Election_Rally", "2024-11-06", "2024-12-06"),
    ("Oct_2024", "2024-10-01", "2024-10-31"),
    ("Sep_2024", "2024-09-01", "2024-09-30"),
    ("Aug_2024", "2024-08-01", "2024-08-31"),
]

SYMBOLS = ["AAPL", "NVDA", "TSLA", "MSFT", "JPM", "GOOGL", "AMZN", "META"]

def run_benchmark(symbol, start_date, end_date):
    """Run benchmark and parse results"""
    cmd = [
        "cargo", "run", "--release", "--bin", "benchmark", "--",
        "--symbol", symbol,
        "--start", start_date,
        "--end", end_date,
        "--strategy", "advanced"
    ]
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        output = result.stdout + result.stderr
        
        # Parse output line: "Return: X.XX% | Net: $XXX.XX | Trades: X"
        match = re.search(r'Return:\s*([-\d.]+)%.*?Net:\s*\$([-\d.]+).*?Trades:\s*(\d+)', output)
        
        if match:
            return {
                'return': float(match.group(1)),
                'net': float(match.group(2)),
                'trades': int(match.group(3)),
                'status': 'SUCCESS'
            }
        else:
            return {'status': 'PARSE_ERROR', 'return': 0, 'net': 0, 'trades': 0}
    except subprocess.TimeoutExpired:
        return {'status': 'TIMEOUT', 'return': 0, 'net': 0, 'trades': 0}
    except Exception as e:
        return {'status': f'ERROR: {e}', 'return': 0, 'net': 0, 'trades': 0}

def main():
    print("=" * 50)
    print("Risk-Based Filter Validation Benchmark")
    print("=" * 50)
    print(f"Results will be saved to: {RESULTS_FILE}")
    print()
    
    total_tests = len(PERIODS) * len(SYMBOLS)
    current = 0
    
    results = []
    
    # Run benchmarks
    for period_name, start_date, end_date in PERIODS:
        for symbol in SYMBOLS:
            current += 1
            print(f"[{current}/{total_tests}] Testing {symbol} ({period_name}: {start_date} to {end_date})...", end=" ")
            
            result = run_benchmark(symbol, start_date, end_date)
            
            if result['status'] == 'SUCCESS':
                print(f"✓ Return: {result['return']:.2f}%, Net: ${result['net']:.2f}, Trades: {result['trades']}")
            else:
                print(f"✗ {result['status']}")
            
            results.append({
                'period': period_name,
                'symbol': symbol,
                'start_date': start_date,
                'end_date': end_date,
                **result
            })
        print()
    
    # Save to CSV
    with open(RESULTS_FILE, 'w', newline='') as f:
        writer = csv.DictWriter(f, fieldnames=['period', 'symbol', 'start_date', 'end_date', 'return', 'net', 'trades', 'status'])
        writer.writeheader()
        writer.writerows(results)
    
    # Generate summary
    successful = [r for r in results if r['status'] == 'SUCCESS']
    
    print("=" * 50)
    print("Benchmark Complete!")
    print("=" * 50)
    print(f"\nResults saved to: {RESULTS_FILE}\n")
    
    if successful:
        total_return = sum(r['return'] for r in successful)
        total_profit = sum(r['net'] for r in successful)
        total_trades = sum(r['trades'] for r in successful)
        winners = sum(1 for r in successful if r['return'] > 0)
        
        print("Summary Statistics:")
        print(f"  Tests Completed: {len(successful)}/{total_tests}")
        print(f"  Average Return: {total_return/len(successful):.2f}%")
        print(f"  Total Net Profit: ${total_profit:.2f}")
        print(f"  Win Rate: {winners/len(successful)*100:.1f}%")
        print(f"  Total Trades: {total_trades}")
        print(f"  Avg Trades per Test: {total_trades/len(successful):.1f}")
    else:
        print("⚠ No successful tests!")
    
    print(f"\nFull results in: {RESULTS_FILE}")

if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
Strategy Comparison: Advanced vs RegimeAdaptive
Tests both strategies across multiple periods to validate adaptive approach
"""

import subprocess
import re
import csv
from datetime import datetime
from pathlib import Path

RESULTS_DIR = Path("benchmark_results")
RESULTS_DIR.mkdir(exist_ok=True)
TIMESTAMP = datetime.now().strftime('%Y%m%d_%H%M%S')

# Test configuration - focus on diverse market conditions
PERIODS = [
    ("Election_Rally", "2024-11-06", "2024-12-06"),
    ("Oct_2024", "2024-10-01", "2024-10-31"),
    ("Sep_2024", "2024-09-01", "2024-09-30"),
]

SYMBOLS = ["AAPL", "NVDA", "TSLA", "MSFT"]  # Representative sample
STRATEGIES = ["advanced", "regime_adaptive"]

def run_benchmark(symbol, start_date, end_date, strategy):
    """Run benchmark and parse results"""
    cmd = [
        "cargo", "run", "--release", "--bin", "benchmark", "--",
        "--symbol", symbol,
        "--start", start_date,
        "--end", end_date,
        "--strategy", strategy
    ]
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        output = result.stdout + result.stderr
        
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
    print("=" * 60)
    print("Strategy Comparison: Advanced vs RegimeAdaptive")
    print("=" * 60)
    print()
    
    results = {strategy: [] for strategy in STRATEGIES}
    
    for strategy in STRATEGIES:
        print(f"\n{'='*60}")
        print(f"Testing {strategy.upper()} Strategy")
        print(f"{'='*60}\n")
        
        total_tests = len(PERIODS) * len(SYMBOLS)
        current = 0
        
        for period_name, start_date, end_date in PERIODS:
            for symbol in SYMBOLS:
                current += 1
                print(f"[{current}/{total_tests}] {symbol} ({period_name})...", end=" ")
                
                result = run_benchmark(symbol, start_date, end_date, strategy)
                
                if result['status'] == 'SUCCESS':
                    print(f"✓ {result['return']:+.2f}% (${result['net']:+.2f}, {result['trades']} trades)")
                else:
                    print(f"✗ {result['status']}")
                
                results[strategy].append({
                    'strategy': strategy,
                    'period': period_name,
                    'symbol': symbol,
                    **result
                })
    
    # Save results
    all_results = []
    for strategy in STRATEGIES:
        all_results.extend(results[strategy])
    
    results_file = RESULTS_DIR / f"strategy_comparison_{TIMESTAMP}.csv"
    with open(results_file, 'w', newline='') as f:
        writer = csv.DictWriter(f, fieldnames=['strategy', 'period', 'symbol', 'return', 'net', 'trades', 'status'])
        writer.writeheader()
        writer.writerows(all_results)
    
    # Generate comparison
    print("\n" + "=" * 60)
    print("COMPARISON SUMMARY")
    print("=" * 60)
    
    for strategy in STRATEGIES:
        successful = [r for r in results[strategy] if r['status'] == 'SUCCESS']
        if successful:
            avg_return = sum(r['return'] for r in successful) / len(successful)
            total_profit = sum(r['net'] for r in successful)
            total_trades = sum(r['trades'] for r in successful)
            winners = sum(1 for r in successful if r['return'] > 0)
            
            print(f"\n{strategy.upper()}:")
            print(f"  Avg Return: {avg_return:+.2f}%")
            print(f"  Total P&L: ${total_profit:+.2f}")
            print(f"  Win Rate: {winners/len(successful)*100:.1f}%")
            print(f"  Total Trades: {total_trades}")
            print(f"  Avg Trades/Test: {total_trades/len(successful):.1f}")
    
    # Period-by-period comparison
    print("\n" + "=" * 60)
    print("PERIOD BREAKDOWN")
    print("=" * 60)
    
    for period_name, _, _ in PERIODS:
        print(f"\n{period_name}:")
        for strategy in STRATEGIES:
            period_results = [r for r in results[strategy] if r['period'] == period_name and r['status'] == 'SUCCESS']
            if period_results:
                total = sum(r['net'] for r in period_results)
                avg = sum(r['return'] for r in period_results) / len(period_results)
                print(f"  {strategy:20s}: ${total:+8.2f} ({avg:+.2f}% avg)")
    
    print(f"\nFull results: {results_file}")

if __name__ == "__main__":
    main()

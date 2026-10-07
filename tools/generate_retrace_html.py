#!/usr/bin/env python3
"""
Generate HTML report from retrace results.
Parses retrace logs and result JSON files to create an HTML report with:
- SSIM scores and pass/fail status
- Shader compile failures
- Mismatch pixel counts
- Screenshots/diffs if available
"""
import json
import os
import re
import sys
from pathlib import Path
from typing import Dict, List, Optional, Any
from datetime import datetime

def parse_retrace_log(log_path: Path) -> Dict:
    """Parse retrace log file for errors and statistics."""
    result = {
        "shader_failures": [],
        "ssim": None,
        "mismatch_pixels": None,
        "golden_path": None,
        "errors": [],
        "shader_failures_count": 0,
    }

    if not log_path.exists():
        return result

    content = log_path.read_text()

    # Parse SSIM
    ssim_match = re.search(r'ssim=([\d.]+)', content)
    if ssim_match:
        result["ssim"] = float(ssim_match.group(1))

    # Parse mismatch pixels
    mismatch_match = re.search(r'mismatchPixels=(\d+)', content)
    if mismatch_match:
        result["mismatch_pixels"] = int(mismatch_match.group(1))

    # Parse golden path
    golden_match = re.search(r'matchedGoldenPath=([^\s]+)', content)
    if golden_match:
        result["golden_path"] = golden_match.group(1)

    # Parse shader failures
    shader_fails = re.findall(r'shader (\d+) failed to compile: (.*?)(?=\n\[RustRenderer\]|$)', content, re.DOTALL)
    for shader_num, error in shader_fails:
        result["shader_failures"].append({
            "shader": int(shader_num),
            "error": error.strip()[:500]
        })
    result["shader_failures_count"] = len(shader_fails)

    # General errors
    errors = re.findall(r'error: (.+)', content)
    result["errors"] = errors[:20]

    return result


def parse_result_json(json_path: Path) -> Dict:
    """Parse retrace result JSON file."""
    if not json_path.exists():
        return {}
    try:
        return json.loads(json_path.read_text())
    except json.JSONDecodeError:
        return {}


def scan_results_dir(results_dir: Path) -> List[Dict]:
    """Scan all result JSON files in the results directory."""
    results = []
    for json_file in sorted(results_dir.glob("*.json")):
        data = parse_result_json(Path(json_file))
        if data:
            data["file"] = str(json_file.relative_to(json_file.parent.parent))
            results.append(data)
    return results


def get_shader_failures_from_log(log_path: Path) -> List[Dict]:
    """Extract shader compilation failures from retrace log."""
    failures = []
    if not log_path.exists():
        return failures

    content = log_path.read_text()
    # Pattern: shader N failed to compile: error message
    pattern = r'shader (\d+) failed to compile: (.*?)(?=\n\[RustRenderer\]|$)'
    matches = re.findall(r'shader (\d+) failed to compile: (.*?)(?=\n\[RustRenderer\]|$)', content, re.DOTALL)
    for shader_num, error in matches:
        error_clean = error.strip()
        if len(error_clean) > 500:
            error_clean = error_clean[:500] + "..."
        failures.append({
            "shader": int(shader_num),
            "error": error_clean
        })
    return failures


def generate_html_report(results_dir: Path, logs_dir: Path, output_path: Path):
    """Generate HTML report from retrace results."""
    results_dir = Path(results_dir)
    logs_dir = Path(logs_dir)

    # Parse all logs
    log_files = list(logs_dir.glob("*-retrace.log"))
    log_files.append(logs_dir / "gl-smoke.log")

    all_shader_failures = []
    all_errors = []
    test_results = {}

    for log_file in log_files:
        if not log_file.exists():
            continue
        result = parse_retrace_log(log_file)
        test_name = log_file.stem.replace("-retrace", "").replace(".log", "")
        test_results[test_name] = result

        if result["shader_failures"]:
            for sf in result["shader_failures"]:
                sf["test"] = test_name
                all_shader_failures.append(sf)

        if result["errors"]:
            for err in result["errors"]:
                all_errors.append({"test": test_name, "error": err})

    # Also check unit test log
    unit_test_log = Path("target/reports/unit-tests.log")
    unit_tests_passed = True
    if unit_test_log.exists():
        content = unit_test_log.read_text()
        if "FAILED" in content or "FAILED" in content:
            unit_tests_passed = False

    # Read lane comparison if exists
    lane_comparison = ""
    lane_comparison_path = Path("target/reports/lane-comparison.md")
    if lane_comparison_path.exists():
        lane_comparison = lane_comparison_path.read_text()

    # Read lane comparison from gl-smoke
    lane_comparison_md = Path("target/reports/lane-comparison.md")
    lane_comparison_content = lane_comparison_md.read_text() if lane_comparison_md.exists() else ""

    # Collect all shader failures
    total_shader_failures = sum(len(r.get("shader_failures", [])) for r in test_results.values())

    # Generate HTML
    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>RustGL Retrace Report</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; background: #f5f5f5; }}
        .container {{ max-width: 1200px; margin: 0 auto; background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        h1 {{ color: #333; border-bottom: 2px solid #4CAF50; padding-bottom: 10px; }}
        h2 {{ color: #555; margin-top: 30px; }}
        .summary {{ display: flex; gap: 20px; margin: 20px 0; flex-wrap: wrap; }}
        .card {{ background: #f8f9fa; padding: 15px; border-radius: 6px; min-width: 150px; flex: 1; }}
        .card h3 {{ margin: 0 0 10px 0; font-size: 14px; color: #666; }}
        .card .value {{ font-size: 28px; font-weight: bold; }}
        .pass {{ color: #4CAF50; }}
        .fail {{ color: #f44336; }}
        .warn {{ color: #ff9800; }}
        table {{ width: 100%; border-collapse: collapse; margin-top: 20px; }}
        th, td {{ padding: 12px; text-align: left; border-bottom: 1px solid #eee; }}
        th {{ background: #f5f5f5; font-weight: 600; }}
        tr:hover {{ background: #f9f9f9; }}
        .fail {{ color: #f44336; font-weight: bold; }}
        .pass {{ color: #4CAF50; }}
        .error {{ color: #f44336; font-family: monospace; font-size: 13px; }}
        .shader-fail {{ background: #fff3e0; }}
        .log-link {{ color: #2196F3; text-decoration: none; }}
        .log-link:hover {{ text-decoration: underline; }}
        .section {{ margin-top: 30px; }}
        pre {{ background: #263238; color: #aed581; padding: 15px; border-radius: 4px; overflow-x: auto; font-size: 12px; }}
        .badge {{ display: inline-block; padding: 3px 8px; border-radius: 3px; font-size: 12px; font-weight: bold; }}
        .badge-pass {{ background: #e8f5e9; color: #2e7d32; }}
        .badge-fail {{ background: #fbe9e7; color: #c62828; }}
        .badge-warn {{ background: #fff3e0; color: #e65100; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>RustGL Retrace Report</h1>
        <p>Generated on {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} UTC</p>

        <div class="summary">
            <div class="card">
                <h3>Shader Compile Failures</h3>
                <div class="value {'fail' if total_shader_failures > 0 else 'pass'}">{total_shader_failures}</div>
            </div>
            <div class="card">
                <h3>Test Cases</h3>
                <div class="value">{len(test_results)}</div>
            </div>
            <div class="card">
                <h3>Unit Tests</h3>
                <div class="value {'pass' if unit_tests_passed else 'fail'}">{'PASS' if unit_tests_passed else 'FAIL'}</div>
            </div>
            <div class="card">
                <h3>Total Errors</h3>
                <div class="value {'fail' if all_errors else 'pass'}">{len(all_errors)}</div>
            </div>
        </div>

        <div class="section">
            <h2>Test Results</h2>
            <table>
                <thead>
                    <tr>
                        <th>Test</th>
                        <th>SSIM</th>
                        <th>Mismatch Pixels</th>
                        <th>Shader Failures</th>
                        <th>Errors</th>
                        <th>Status</th>
                    </tr>
                </thead>
                <tbody>
"""

    # Add test results rows
    for test_name, result in test_results.items():
        ssim = result.get("ssim")
        mismatch = result.get("mismatch_pixels")
        failures = result.get("shader_failures_count", 0)
        errors = len(result.get("errors", []))
        status = "PASS" if failures == 0 and len(result.get("errors", [])) == 0 else "FAIL"
        status_class = "pass" if status == "PASS" else "fail"

        ssim_str = f"{ssim:.6f}" if ssim is not None else "N/A"
        mismatch_str = f"{mismatch:,}" if mismatch is not None else "N/A"

        html += f"""
                <tr>
                    <td>{test_name}</td>
                    <td>{ssim_str}</td>
                    <td>{mismatch_str}</td>
                    <td>{failures}</td>
                    <td>{len(result.get('errors', []))}</td>
                    <td><span class="badge {'badge-pass' if status == 'PASS' else 'badge-fail'}">{status}</span></td>
                </tr>
"""

    html += """
                </tbody>
            </table>
        </div>
"""

    # Shader failures table
    if all_shader_failures:
        html += """
        <div class="section">
            <h2>Shader Compilation Failures</h2>
            <table>
                <thead>
                    <tr>
                        <th>Test</th>
                        <th>Shader #</th>
                        <th>Error</th>
                    </tr>
                </thead>
                <tbody>
"""
        for sf in all_shader_failures:
            html += f"""
                    <tr class="shader-fail">
                        <td>{sf['test']}</td>
                        <td>{sf['shader']}</td>
                        <td class="error">{sf['error']}</td>
                    </tr>
"""
        html += """
                </tbody>
            </table>
        </div>
"""

    # Errors section
    if all_errors:
        html += """
        <div class="section">
            <h2>Other Errors</h2>
            <table>
                <thead>
                    <tr>
                        <th>Test</th>
                        <th>Error</th>
                    </tr>
                </thead>
                <tbody>
"""
        for err in all_errors:
            html += f"""
                    <tr>
                        <td>{err['test']}</td>
                        <td class="error">{err['error']}</td>
                    </tr>
"""
        html += """
                </tbody>
            </table>
        </div>
"""

    # Lane comparison
    if lane_comparison_content:
        html += f"""
        <div class="section">
            <h2>Lane Comparison</h2>
            <pre>{lane_comparison_content}</pre>
        </div>
"""

    # Unit tests
    html += f"""
        <div class="section">
            <h2>Unit Tests</h2>
            <div class="card">
                <h3>Unit Tests</h3>
                <div class="value {'pass' if unit_tests_passed else 'fail'}">{'PASS' if unit_tests_passed else 'FAIL'}</div>
            </div>
        </div>

        <div class="section">
            <h2>Lane Comparison</h2>
            <pre>{lane_comparison_content}</pre>
        </div>
"""

    html += """
    </div>
</body>
</html>
"""

    output_path.write_text(html)
    print(f"Report written to {output_path}")


if __name__ == "__main__":
    results_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("target/trace-replay/results")
    logs_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("target/reports")
    output_path = Path(sys.argv[3]) if len(sys.argv) > 3 else Path("target/reports/retrace-report.html")

    generate_html_report(Path("target/trace-replay/results"), Path("target/reports"), Path("target/reports/retrace-report.html"))
#!/usr/bin/env python3
"""
Download and filter PCI IDs database for NVRC

Filters to:
- NVIDIA GPUs: Ampere+ (22xx, 23xx, 29xx and higher)
- NVIDIA NVSwitch: ALL generations (by device name)
- Mellanox: ALL devices (NIC infrastructure)

Output: Clean data file (no comments) for embedding in Rust binary
"""

import sys
import urllib.request
from typing import Set

# Configuration
PCI_IDS_URL = "https://pci-ids.ucw.cz/v2.2/pci.ids"
DEFAULT_OUTPUT = "src/pci_ids_embedded.txt"

# NVIDIA vendor ID
NVIDIA_VENDOR = "10de"
MELLANOX_VENDOR = "15b3"


def should_include_nvidia_device(device_id: str, device_line: str) -> bool:
    """
    Determine if an NVIDIA device should be included

    Include:
    - All NVSwitch devices (by name)
    - GPUs from Ampere generation onwards (22xx, 23xx, 29xx+)
    """
    # Check if it's an NVSwitch (any generation)
    if "nvswitch" in device_line.lower():
        return True

    # Check device ID range for GPUs
    # Ampere: 22xx (GA102+)
    # Hopper: 23xx
    # Blackwell: 29xx-2fxx
    # Future: 3xxx+
    if device_id.startswith('22'):
        return True
    if device_id.startswith('23'):
        return True
    if device_id.startswith('2') and device_id[1] in '9abcdef':
        return True
    if device_id[0] in '3456789abcdef':
        return True

    return False


def filter_pci_ids(input_file, output_file):
    """Filter PCI IDs to NVIDIA (Ampere+) and Mellanox only"""

    current_vendor = None
    nvidia_devices = []
    output_lines = []
    device_counts = {'hopper': 0, 'ampere': 0, 'blackwell': 0, 'nvswitch': 0}

    for line in input_file:
        line = line.rstrip('\n')

        # Skip comments and empty lines
        if line.startswith('#') or not line.strip():
            continue

        # Vendor line (no leading tab)
        if line and not line.startswith('\t'):
            # Check if it's a vendor line (4 hex digits + spaces)
            parts = line.split(None, 1)
            if len(parts) >= 1 and len(parts[0]) == 4:
                vendor_id = parts[0]

                # Save current vendor
                if vendor_id == NVIDIA_VENDOR:
                    current_vendor = 'nvidia'
                    nvidia_vendor_line = line
                    nvidia_devices = []  # Reset for this vendor
                elif vendor_id == MELLANOX_VENDOR:
                    # Print any collected NVIDIA devices first
                    if nvidia_devices:
                        output_lines.append(nvidia_vendor_line)
                        output_lines.extend(nvidia_devices)
                        nvidia_devices = []
                    current_vendor = 'mellanox'
                    output_lines.append(line)
                else:
                    # Different vendor - flush NVIDIA if needed
                    if nvidia_devices:
                        output_lines.append(nvidia_vendor_line)
                        output_lines.extend(nvidia_devices)
                        nvidia_devices = []
                    current_vendor = None
                continue

        # Device line (single tab)
        if line.startswith('\t') and not line.startswith('\t\t'):
            if current_vendor == 'nvidia':
                # Extract device ID (first 4 hex chars after tab)
                device_id = line.split()[0]

                if should_include_nvidia_device(device_id, line):
                    nvidia_devices.append(line)

                    # Track generation for statistics
                    if 'nvswitch' in line.lower():
                        device_counts['nvswitch'] += 1
                    elif device_id.startswith('22'):
                        device_counts['ampere'] += 1
                    elif device_id.startswith('23'):
                        device_counts['hopper'] += 1
                    elif device_id.startswith('2') and device_id[1] in '9abcdef':
                        device_counts['blackwell'] += 1
            elif current_vendor == 'mellanox':
                output_lines.append(line)
            continue

        # Subsystem line (double tab)
        if line.startswith('\t\t'):
            if current_vendor == 'nvidia' and nvidia_devices:
                # Include subsystem if we included the parent device
                nvidia_devices.append(line)
            elif current_vendor == 'mellanox':
                output_lines.append(line)
            continue

    # Flush any remaining NVIDIA devices
    if nvidia_devices:
        output_lines.append(nvidia_vendor_line)
        output_lines.extend(nvidia_devices)

    # Write output
    with open(output_file, 'w') as f:
        for line in output_lines:
            f.write(line + '\n')

    return len(output_lines), device_counts


def main():
    output_file = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_OUTPUT

    print(f"Downloading PCI IDs database from {PCI_IDS_URL}...")

    # Download
    try:
        with urllib.request.urlopen(PCI_IDS_URL) as response:
            content = response.read().decode('utf-8')
    except Exception as e:
        print(f"Error downloading: {e}", file=sys.stderr)
        sys.exit(1)

    if not content:
        print("Error: Downloaded file is empty", file=sys.stderr)
        sys.exit(1)

    original_lines = content.count('\n')
    print(f"Downloaded {original_lines} lines")

    # Filter
    print(f"Filtering to NVIDIA (Ampere+, NVSwitch) and Mellanox...")
    filtered_lines, counts = filter_pci_ids(content.splitlines(), output_file)

    print(f"\nFiltering complete!")
    print(f"  Original: {original_lines} lines")
    print(f"  Filtered: {filtered_lines} lines (no comments)")
    print(f"  Output: {output_file}")
    print(f"\nNVIDIA Devices:")
    print(f"  - Ampere GPUs (22xx): {counts['ampere']}")
    print(f"  - Hopper GPUs (23xx): {counts['hopper']}")
    print(f"  - Blackwell GPUs (29xx+): {counts['blackwell']}")
    print(f"  - NVSwitch (all gen): {counts['nvswitch']}")
    print(f"\nTotal NVIDIA: {sum(counts.values())}")


if __name__ == '__main__':
    main()


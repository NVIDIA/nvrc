#!/bin/bash

# Script to download PCI IDs database and filter for NVIDIA Hopper+ and Mellanox vendors only
# NVIDIA filtering includes only:
# - Hopper (GH100): Device IDs 23xx (NOT 22xx which is Ampere GA102)
# - Blackwell (GB100/GB200): Device IDs 29xx, 2axx, 2bxx, 2cxx, 2dxx, 2exx, 2fxx
# - Future generations with device IDs 3xxx and higher
# Usage: ./download_filtered_pci_ids.sh [output_file]
# Default output: tests/data/pci.ids

set -euo pipefail

# Configuration
PCI_IDS_URL="https://pci-ids.ucw.cz/v2.2/pci.ids"
DEFAULT_OUTPUT="tests/data/pci.ids"
OUTPUT_FILE="${1:-$DEFAULT_OUTPUT}"
TEMP_FILE=$(mktemp)

echo "Downloading PCI IDs database and filtering for NVIDIA Hopper+ and Mellanox vendors..."
echo "Source: ${PCI_IDS_URL}"
echo "Output: ${OUTPUT_FILE}"

# Create output directory if it doesn't exist
OUTPUT_DIR=$(dirname "$OUTPUT_FILE")
if [[ ! -d "$OUTPUT_DIR" ]]; then
    echo "Creating directory: ${OUTPUT_DIR}"
    mkdir -p "$OUTPUT_DIR"
fi

# Download the PCI IDs file
if ! curl -s -L "$PCI_IDS_URL" -o "$TEMP_FILE"; then
    echo "Error: Failed to download PCI IDs database" >&2
    rm -f "$TEMP_FILE"
    exit 1
fi

# Check if download was successful
if [[ ! -s "$TEMP_FILE" ]]; then
    echo "Error: Downloaded file is empty" >&2
    rm -f "$TEMP_FILE"
    exit 1
fi

ORIGINAL_SIZE=$(wc -l < "$TEMP_FILE")

# AWK script to extract header comments, NVIDIA Hopper+ generation devices, and Mellanox vendors
# NVIDIA Device ID ranges for Hopper and newer:
# - Hopper (GH100): 23xx (NOT 22xx which is Ampere GA102)
# - Blackwell (GB100/GB200): 29xx, 2axx, 2bxx, 2cxx, 2dxx, 2exx, 2fxx
# - Future generations: 3xxx and higher
awk '
BEGIN {
    in_nvidia = 0
    in_mellanox = 0
    in_other_vendor = 0
    nvidia_vendor_printed = 0
    last_nvidia_device_printed = 0
}

# Keep all header comments (lines starting with #)
/^#/ {
    print
    next
}

# Empty lines - print them
/^$/ {
    print
    next
}

# NVIDIA vendor line (10de) - we will print this only if we find Hopper+ devices
/^10de  / {
    nvidia_vendor_line = $0
    in_nvidia = 1
    in_mellanox = 0
    in_other_vendor = 0
    nvidia_vendor_printed = 0
    last_nvidia_device_printed = 0
    next
}

# Mellanox vendor line (15b3)
/^15b3  / {
    in_nvidia = 0
    in_mellanox = 1
    in_other_vendor = 0
    print
    next
}

# Any other vendor line (starts with hex ID followed by two spaces)
/^[0-9a-f][0-9a-f][0-9a-f][0-9a-f]  / {
    # This is a new vendor that is not NVIDIA or Mellanox
    in_nvidia = 0
    in_mellanox = 0
    in_other_vendor = 1
    last_nvidia_device_printed = 0
    next  # Skip this vendor and all its devices
}

# Device lines (start with tab but not double tab)
/^\t[^\t]/ {
    if (in_nvidia) {
        # Extract device ID (first 4 hex chars after tab)
        device_id = substr($1, 1, 4)
        
        # Check if this is a Hopper or newer device
        # Hopper: 23xx (NOT 22xx which is Ampere)
        # Blackwell: 29xx-2fxx  
        # Future: 3xxx and higher
        is_hopper_plus = 0
        
        # Check for Hopper generation (23xx only, exclude 22xx Ampere)
        if (device_id ~ /^23/) {
            is_hopper_plus = 1
        }
        # Check for Blackwell generation (29xx-2fxx)
        else if (device_id ~ /^2[9a-f]/) {
            is_hopper_plus = 1
        }
        # Check for future generations (3xxx and higher)
        else if (device_id ~ /^[3-9a-f]/) {
            is_hopper_plus = 1
        }
        
        if (is_hopper_plus) {
            # Print NVIDIA vendor line if we haven'\''t already
            if (!nvidia_vendor_printed) {
                print nvidia_vendor_line
                nvidia_vendor_printed = 1
            }
            print
            last_nvidia_device_printed = 1
        } else {
            last_nvidia_device_printed = 0
        }
    } else if (in_mellanox) {
        print
    }
    next
}

# Subsystem lines (start with double tab)
/^\t\t/ {
    if (in_nvidia && last_nvidia_device_printed) {
        # Only print subsystem lines if we printed the parent device
        print
    } else if (in_mellanox) {
        print
    }
    next
}

# Device classes and other sections (no leading tab or hex)
/^C [0-9a-f]/ {
    # Device class section - skip entirely
    in_nvidia = 0
    in_mellanox = 0
    in_other_vendor = 1
    last_nvidia_device_printed = 0
    next
}

# Skip everything else when we are in other vendor or class sections
{
    if (in_other_vendor) {
        next
    }
    # If we get here and we are in NVIDIA or Mellanox section, print it
    if (in_nvidia || in_mellanox) {
        print
    }
}
' "$TEMP_FILE" > "$OUTPUT_FILE"

# Check if filtering was successful
if [[ ! -s "$OUTPUT_FILE" ]]; then
    echo "Error: Filtered file is empty" >&2
    rm -f "$TEMP_FILE"
    exit 1
fi

# Get filtered file statistics
FILTERED_SIZE=$(wc -l < "$OUTPUT_FILE")
NVIDIA_DEVICES=$(grep -c "^\t[0-9a-f]" "$OUTPUT_FILE" || echo "0")
TOTAL_VENDORS=$(grep -c "^[0-9a-f][0-9a-f][0-9a-f][0-9a-f]  " "$OUTPUT_FILE" || echo "0")

echo "Filtering completed successfully!"
echo "Original lines: ${ORIGINAL_SIZE}"
echo "Filtered lines: ${FILTERED_SIZE}"
echo "Vendors kept: ${TOTAL_VENDORS} (NVIDIA Hopper+ + Mellanox)"
echo "NVIDIA Hopper+ devices: ${NVIDIA_DEVICES}"

# Show vendor information and generation breakdown
grep "^[0-9a-f][0-9a-f][0-9a-f][0-9a-f]  " "$OUTPUT_FILE" | while read -r line; do
    vendor_id=$(echo "$line" | awk '{print $1}')
    vendor_name=$(echo "$line" | cut -d' ' -f3-)
    device_count=$(awk -v vid="$vendor_id" 'BEGIN{count=0} /^\t[0-9a-f]/ && prev_vendor==vid {count++} /^[0-9a-f][0-9a-f][0-9a-f][0-9a-f]  / {prev_vendor=$1} END{print count}' "$OUTPUT_FILE")
    
    if [[ "$vendor_id" == "10de" ]]; then
        echo "${vendor_id}: ${vendor_name} (${device_count} Hopper+ generation devices)"
        
        # Show breakdown by generation
        hopper_count=$(awk '/^\t23/ {count++} END {print count+0}' "$OUTPUT_FILE")
        blackwell_count=$(awk '/^\t2[9a-f]/ {count++} END {print count+0}' "$OUTPUT_FILE")
        future_count=$(awk '/^\t[3-9a-f]/ {count++} END {print count+0}' "$OUTPUT_FILE")
        
        echo "  - Hopper (23xx): ${hopper_count} devices"
        echo "  - Blackwell (29xx-2fxx): ${blackwell_count} devices"
        if [[ "$future_count" -gt 0 ]]; then
            echo "  - Future generations (3xxx+): ${future_count} devices"
        fi
    else
        echo "${vendor_id}: ${vendor_name} (${device_count} devices)"
    fi
done

# Cleanup
rm -f "$TEMP_FILE"

echo "PCI IDs database successfully downloaded and filtered to: ${OUTPUT_FILE}"
echo ""
echo "Included NVIDIA generations:"
echo "  - Hopper (GH100): Device IDs 23xx"
echo "  - Blackwell (GB100/GB200): Device IDs 29xx-2fxx"
echo "  - Future generations: Device IDs 3xxx and higher"
echo ""
echo "Excluded: All pre-Hopper NVIDIA devices (00xx-22xx, 24xx-28xx)"

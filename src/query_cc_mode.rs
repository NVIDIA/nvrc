#[cfg(feature = "confidential")]
mod confidential {

    use anyhow::{Context, Result};
    use std::collections::HashMap;
    use std::fs::File;
    use std::ptr;

    // For mmap functionality
    use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};

    use super::super::NVRC;
    use crate::pci_ids::DeviceType;

    // Embed the filtered PCI IDs database at compile time
    const EMBEDDED_PCI_IDS: &str = include_str!("pci_ids_embedded.txt");

    // GPU Architecture enumeration with associated CC register offsets
    #[derive(Debug, PartialEq, Clone)]
    pub enum GpuArchitecture {
        Hopper,
        Blackwell,
        Unknown,
    }

    impl GpuArchitecture {
        /// CC state mask used to extract CC mode bits from register value
        pub const CC_STATE_MASK: u32 = 0x3;

        /// CC Mode register values and their corresponding mode strings
        pub const CC_MODE_LOOKUP: &'static [(u32, &'static str)] =
            &[(0x1, "on"), (0x3, "devtools"), (0x0, "off")];

        /// Get the CC register offset for this GPU architecture
        pub fn cc_register(&self) -> Result<u64> {
            match self {
            GpuArchitecture::Hopper => Ok(0x001182cc),
            GpuArchitecture::Blackwell => Ok(0x590),
            GpuArchitecture::Unknown => {
                Err(anyhow::anyhow!(
                    "Cannot determine CC register for unknown GPU architecture. This is required for safe hardware access."
                ))
            }
        }
        }

        /// Parse CC mode from register value using the common lookup table
        pub fn parse_cc_mode(&self, reg_value: u32) -> Result<String> {
            if matches!(self, GpuArchitecture::Unknown) {
                return Err(anyhow::anyhow!(
                    "Cannot parse CC mode for unknown GPU architecture."
                ));
            }

            let cc_state = reg_value & Self::CC_STATE_MASK;
            let mode = Self::CC_MODE_LOOKUP
                .iter()
                .find(|(value, _)| *value == cc_state)
                .map(|(_, mode)| *mode)
                .unwrap_or("off"); // Default to "off" for unknown states

            Ok(mode.to_string())
        }
    }

    /// Get PCI IDs database - uses embedded data by default for self-contained operation
    fn get_pci_ids_database() -> Result<HashMap<u16, String>> {
        parse_pci_database_content(EMBEDDED_PCI_IDS)
    }

    /// Parse PCI database content (from file or embedded data)
    fn parse_pci_database_content(content: &str) -> Result<HashMap<u16, String>> {
        let mut nvidia_devices = HashMap::new();
        let mut in_nvidia_section = false;

        for line in content.lines() {
            if line.starts_with("10de  NVIDIA Corporation") {
                in_nvidia_section = true;
                continue;
            }

            if in_nvidia_section {
                if line.starts_with('\t') && !line.starts_with("\t\t") {
                    // Device entry: "\t<device_id>  <device_name>"
                    if let Some(parts) = line.strip_prefix('\t') {
                        if let Some((id_str, name)) = parts.split_once("  ") {
                            if let Ok(device_id) = u16::from_str_radix(id_str, 16) {
                                nvidia_devices.insert(device_id, name.to_string());
                            }
                        }
                    }
                } else if line.starts_with("\t\t") {
                    // Subsystem entry (skip these)
                    continue;
                } else if !line.starts_with('\t') && !line.is_empty() && !line.starts_with('#') {
                    // End of NVIDIA section (new vendor)
                    break;
                }
            }
        }

        Ok(nvidia_devices)
    }

    fn classify_gpu_architecture(device_name: &str) -> GpuArchitecture {
        let name_lower = device_name.to_lowercase();

        // Hopper architecture patterns
        if name_lower.contains("h100")
            || name_lower.contains("h800")
            || name_lower.contains("hopper")
            || name_lower.contains("gh100")
        {
            return GpuArchitecture::Hopper;
        }

        // Blackwell architecture patterns
        if name_lower.contains("b100")
            || name_lower.contains("b200")
            || name_lower.contains("blackwell")
            || name_lower.contains("gb100")
            || name_lower.contains("gb200")
        {
            return GpuArchitecture::Blackwell;
        }

        GpuArchitecture::Unknown
    }

    fn get_gpu_architecture_by_device_id(device_id: u16, bdf: &str) -> Result<GpuArchitecture> {
        debug!("GPU BDF {} has device ID: 0x{:04x}", bdf, device_id);

        let pci_db =
            get_pci_ids_database().with_context(|| "Failed to get embedded PCI database")?;

        if let Some(device_name) = pci_db.get(&device_id) {
            let architecture = classify_gpu_architecture(device_name);
            if architecture == GpuArchitecture::Unknown {
                return Err(anyhow::anyhow!(
                "Device 0x{:04x} ('{}') at BDF {} is not a recognized GPU architecture (Hopper/Blackwell). Cannot determine correct CC register offset.",
                device_id, device_name, bdf
            ));
            }

            Ok(architecture)
        } else {
            Err(anyhow::anyhow!(
            "Device ID 0x{:04x} not found in embedded PCI database. Cannot determine GPU architecture for BDF {}.",
            device_id, bdf
        ))
        }
    }

    impl NVRC {
        /// Query CC mode by reading BAR0 memory mapped register
        ///
        /// Reference:
        /// - https://github.com/NVIDIA/gpu-admin-tools/blob/main/nvidia_gpu_tools.py
        ///   function: query_cc_mode_hopper()
        fn query_cc_mode_bar0(&self, bdf: &str, device_id: u16) -> Result<String> {
            let resource_path = format!("/sys/bus/pci/devices/{bdf}/resource0");
            debug!("Reading BAR0 resource for BDF {}: {}", bdf, resource_path);

            // Detect GPU architecture to determine which register to use
            let architecture = get_gpu_architecture_by_device_id(device_id, bdf)
                .with_context(|| format!("Failed to detect GPU architecture for BDF: {}", bdf))?;

            let cc_register = architecture.cc_register().with_context(|| {
                format!(
                    "Failed to get CC register for GPU architecture {:?}",
                    architecture
                )
            })?;

            debug!(
                "GPU BDF {} detected as {:?}, using CC register 0x{:x}",
                bdf, architecture, cc_register
            );

            let file = File::open(&resource_path)
                .with_context(|| format!("Failed to open BAR0 resource file for BDF: {bdf}"))?;

            // Get page size for mmap alignment
            let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;

            // Calculate aligned offset and the offset within the page
            let aligned_offset = (cc_register as usize / page_size) * page_size;
            let offset_in_page = cc_register as usize - aligned_offset;

            // Map a page starting from aligned offset
            let map_size = page_size;
            let mapped_ptr = unsafe {
                mmap(
                    None,
                    std::num::NonZeroUsize::new(map_size).unwrap(),
                    ProtFlags::PROT_READ,
                    MapFlags::MAP_SHARED,
                    &file,
                    aligned_offset as i64,
                )
                .with_context(|| format!("Failed to mmap BAR0 resource for BDF: {bdf}"))?
            };

            let result = unsafe {
                // Calculate the actual register address within the mapped region
                let reg_ptr = mapped_ptr
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset_in_page)
                    .cast::<u32>();

                // Read the 32-bit register value
                let reg_value = ptr::read_volatile(reg_ptr);

                // Parse CC mode from register value - fail if parsing fails
                let mode = architecture.parse_cc_mode(reg_value).with_context(|| {
                    format!(
                        "Failed to parse CC mode from register value 0x{:x} for BDF: {}",
                        reg_value, bdf
                    )
                })?;

                debug!(
                    "CC mode for BDF {} (via BAR0): {} (0x{:x}) [arch: {:?}]",
                    bdf, mode, reg_value, architecture
                );

                mode
            };

            // Unmap the memory
            unsafe {
                munmap(mapped_ptr, map_size)
                    .with_context(|| format!("Failed to unmap BAR0 resource for BDF: {bdf}"))?;
            }

            Ok(result)
        }
        pub fn query_gpu_cc_mode(&mut self) -> Result<()> {
            let mut mode: Option<String> = None;

            let gpu_devices: Vec<_> = self
                .nvidia_devices
                .iter()
                .filter(|d| matches!(d.device_type, DeviceType::Gpu))
                .collect();

            if gpu_devices.is_empty() {
                debug!("No GPUs found, skipping CC mode query");
                return Ok(());
            }

            for gpu_device in gpu_devices {
                let device_id = gpu_device.device_id;
                let bdf = &gpu_device.bdf;

                // Query CC mode directly via BAR0
                let current_mode = self
                    .query_cc_mode_bar0(bdf, device_id)
                    .with_context(|| format!("Failed to query CC mode via BAR0 for BDF: {bdf}"))?;

                match &mode {
                    Some(m) if m != &current_mode => {
                        return Err(anyhow::anyhow!(
                            "Inconsistent CC mode detected: {} has mode '{}', expected '{}'",
                            bdf,
                            current_mode,
                            m
                        ));
                    }
                    _ => mode = Some(current_mode),
                }
            }
            self.gpu_cc_mode = mode;

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            classify_gpu_architecture, get_pci_ids_database, parse_pci_database_content,
            GpuArchitecture, NVRC,
        };

        #[test]
        fn test_gpu_architecture_classification() {
            // Test Hopper classification
            assert_eq!(
                classify_gpu_architecture("NVIDIA H100 PCIe"),
                GpuArchitecture::Hopper
            );
            assert_eq!(
                classify_gpu_architecture("NVIDIA H800 SXM"),
                GpuArchitecture::Hopper
            );
            assert_eq!(
                classify_gpu_architecture("NVIDIA GH100 [H100 SXM5]"),
                GpuArchitecture::Hopper
            );
            assert_eq!(
                classify_gpu_architecture("hopper test card"),
                GpuArchitecture::Hopper
            );

            // Test Blackwell classification
            assert_eq!(
                classify_gpu_architecture("NVIDIA B100"),
                GpuArchitecture::Blackwell
            );
            assert_eq!(
                classify_gpu_architecture("NVIDIA B200 SXM"),
                GpuArchitecture::Blackwell
            );
            assert_eq!(
                classify_gpu_architecture("NVIDIA GB100"),
                GpuArchitecture::Blackwell
            );
            assert_eq!(
                classify_gpu_architecture("blackwell test"),
                GpuArchitecture::Blackwell
            );

            // Test unknown classification
            assert_eq!(
                classify_gpu_architecture("NVIDIA GeForce RTX 4090"),
                GpuArchitecture::Unknown
            );
            assert_eq!(
                classify_gpu_architecture("NVIDIA Tesla V100"),
                GpuArchitecture::Unknown
            );
            assert_eq!(
                classify_gpu_architecture("Random GPU"),
                GpuArchitecture::Unknown
            );
        }

        #[test]
        fn test_cc_register_selection() {
            assert_eq!(GpuArchitecture::Hopper.cc_register().unwrap(), 0x001182cc);
            assert_eq!(GpuArchitecture::Blackwell.cc_register().unwrap(), 0x590);

            // Test that Unknown architecture returns an error
            let result = GpuArchitecture::Unknown.cc_register();
            assert!(
                result.is_err(),
                "Unknown architecture should return an error"
            );

            let error_msg = result.unwrap_err().to_string();
            assert!(
                error_msg.contains("unknown GPU architecture"),
                "Error should mention unknown architecture"
            );
        }

        #[test]
        fn test_cc_mode_constants() {
            // Test that CC mode constants are properly defined
            assert_eq!(GpuArchitecture::CC_STATE_MASK, 0x3);
            assert_eq!(GpuArchitecture::Hopper.cc_register().unwrap(), 0x001182cc);
            assert_eq!(GpuArchitecture::Blackwell.cc_register().unwrap(), 0x590);
        }

        #[test]
        fn test_cc_mode_lookup_table() {
            // Test that the lookup table contains expected values
            assert_eq!(GpuArchitecture::CC_MODE_LOOKUP.len(), 3);

            // Test each expected mode
            let lookup_map: std::collections::HashMap<u32, &str> =
                GpuArchitecture::CC_MODE_LOOKUP.iter().copied().collect();

            assert_eq!(lookup_map.get(&0x0), Some(&"off"));
            assert_eq!(lookup_map.get(&0x1), Some(&"on"));
            assert_eq!(lookup_map.get(&0x3), Some(&"devtools"));
        }

        #[test]
        fn test_cc_mode_parsing() {
            // Test CC mode parsing for different architectures
            let hopper = GpuArchitecture::Hopper;
            let blackwell = GpuArchitecture::Blackwell;

            // Test different register values
            assert_eq!(hopper.parse_cc_mode(0x0).unwrap(), "off");
            assert_eq!(hopper.parse_cc_mode(0x1).unwrap(), "on");
            assert_eq!(hopper.parse_cc_mode(0x3).unwrap(), "devtools");
            assert_eq!(hopper.parse_cc_mode(0x2).unwrap(), "off"); // Unknown state defaults to "off"

            // Test that Blackwell behaves the same
            assert_eq!(blackwell.parse_cc_mode(0x0).unwrap(), "off");
            assert_eq!(blackwell.parse_cc_mode(0x1).unwrap(), "on");
            assert_eq!(blackwell.parse_cc_mode(0x3).unwrap(), "devtools");

            // Test that Unknown architecture fails
            let unknown = GpuArchitecture::Unknown;
            assert!(unknown.parse_cc_mode(0x1).is_err());
        }

        #[test]
        fn test_pci_database_parsing() {
            // Test parsing of a minimal PCI database format
            let test_content = r#"
# Test PCI database
10de  NVIDIA Corporation
	2330  GH100 [H100 PCIe]
	2331  GH100 [H100 SXM]
	233a  GH100 [H800 PCIe]
	2b00  GB100 [B100]
	1234  Some Other Device
10df  Some Other Vendor
	5678  Other Vendor Device
"#;

            let result = parse_pci_database_content(test_content);
            assert!(result.is_ok());

            let devices = result.unwrap();
            assert_eq!(devices.len(), 5); // Only NVIDIA devices should be included (including the extra device)

            assert_eq!(devices.get(&0x2330), Some(&"GH100 [H100 PCIe]".to_string()));
            assert_eq!(devices.get(&0x2331), Some(&"GH100 [H100 SXM]".to_string()));
            assert_eq!(devices.get(&0x233a), Some(&"GH100 [H800 PCIe]".to_string()));
            assert_eq!(devices.get(&0x2b00), Some(&"GB100 [B100]".to_string()));
            assert_eq!(devices.get(&0x1234), Some(&"Some Other Device".to_string()));

            // Other vendor devices should not be included
            assert_eq!(devices.get(&0x5678), None);
        }

        #[test]
        fn test_query_gpu_cc_mode_no_gpus() {
            // Test behavior when no GPUs are present
            let mut nvrc = NVRC::default();
            nvrc.nvidia_devices = Vec::new();

            let result = nvrc.query_gpu_cc_mode();
            assert!(result.is_ok(), "Should succeed when no GPUs are present");
            assert_eq!(
                nvrc.gpu_cc_mode, None,
                "CC mode should remain None when no GPUs found"
            );
        }

        #[test]
        fn test_architecture_enum() {
            // Test that our enum variants work as expected
            let arch = GpuArchitecture::Hopper;
            assert_eq!(arch, GpuArchitecture::Hopper);
            assert_ne!(arch, GpuArchitecture::Blackwell);

            // Test cloning
            let arch_clone = arch.clone();
            assert_eq!(arch, arch_clone);

            // Test debug formatting
            let debug_str = format!("{:?}", arch);
            assert!(debug_str.contains("Hopper"));
        }

        #[test]
        fn test_architecture_detection_failure() {
            // Test that Unknown architecture causes proper failure
            let result = GpuArchitecture::Unknown.cc_register();
            assert!(result.is_err(), "Unknown architecture should fail");

            let error = result.unwrap_err();
            let error_msg = error.to_string();
            assert!(
                error_msg.contains("unknown GPU architecture"),
                "Error should mention unknown architecture"
            );
            assert!(
                error_msg.contains("safe hardware access"),
                "Error should mention safety concerns"
            );
        }

        #[test]
        fn test_pci_file_requirement() {
            // Test that embedded data works by default
            let result = get_pci_ids_database();
            assert!(result.is_ok(), "Should succeed with embedded data");

            let devices = result.unwrap();
            assert!(
                !devices.is_empty(),
                "Embedded PCI database should contain devices"
            );
        }

        #[test]
        fn test_get_pci_ids_database_functionality() {
            // Test the PCI IDs database parsing with test content
            let test_content = r#"
# Test PCI database
10de  NVIDIA Corporation
	2330  GH100 [H100 SXM5 80GB]
	2331  GH100 [H100 PCIe]
	2322  GH100 [H800 PCIe]
	2901  GB100 [B200]
	2920  GB100 [TS4 / B100]
"#;

            let result = parse_pci_database_content(test_content);
            assert!(result.is_ok(), "Should successfully parse PCI database");

            let devices = result.unwrap();
            assert_eq!(devices.len(), 5);

            // Verify specific devices
            assert_eq!(
                devices.get(&0x2330),
                Some(&"GH100 [H100 SXM5 80GB]".to_string())
            );
            assert_eq!(devices.get(&0x2331), Some(&"GH100 [H100 PCIe]".to_string()));
            assert_eq!(devices.get(&0x2322), Some(&"GH100 [H800 PCIe]".to_string()));
            assert_eq!(devices.get(&0x2901), Some(&"GB100 [B200]".to_string()));
            assert_eq!(
                devices.get(&0x2920),
                Some(&"GB100 [TS4 / B100]".to_string())
            );
        }

        #[test]
        fn test_get_pci_ids_database_embedded_fallback() {
            // Test behavior with embedded data (default operation)
            let result = get_pci_ids_database();
            assert!(result.is_ok(), "Should succeed with embedded data");

            let devices = result.unwrap();
            assert!(
                !devices.is_empty(),
                "Embedded PCI database should contain devices"
            );

            println!(
                "Embedded PCI database contains {} NVIDIA devices",
                devices.len()
            );

            // Look for some expected NVIDIA devices in embedded data
            let has_nvidia_devices = devices.values().any(|name| {
                let name_lower = name.to_lowercase();
                name_lower.contains("nvidia")
                    || name_lower.contains("geforce")
                    || name_lower.contains("quadro")
                    || name_lower.contains("tesla")
                    || name_lower.contains("h100")
                    || name_lower.contains("h800")
            });

            assert!(
                has_nvidia_devices,
                "Embedded database should contain recognizable NVIDIA devices"
            );
        }

        #[test]
        fn test_embedded_pci_database_content() {
            // Test that the embedded database can be parsed correctly
            let result = get_pci_ids_database();
            assert!(
                result.is_ok(),
                "Should successfully parse embedded PCI database"
            );

            let devices = result.unwrap();
            assert!(
                devices.len() > 70,
                "Embedded database should contain many devices (found {})",
                devices.len()
            );

            println!("Embedded database contains {} devices", devices.len());

            // Test some specific architectures if they exist
            let hopper_devices: Vec<_> = devices
                .iter()
                .filter(|(_, name)| {
                    let name_lower = name.to_lowercase();
                    name_lower.contains("h100")
                        || name_lower.contains("h800")
                        || name_lower.contains("gh100")
                })
                .collect();

            let blackwell_devices: Vec<_> = devices
                .iter()
                .filter(|(_, name)| {
                    let name_lower = name.to_lowercase();
                    name_lower.contains("b100")
                        || name_lower.contains("b200")
                        || name_lower.contains("gb100")
                        || name_lower.contains("gb200")
                })
                .collect();

            if !hopper_devices.is_empty() {
                println!(
                    "Found {} Hopper devices in embedded database",
                    hopper_devices.len()
                );
            }
            if !blackwell_devices.is_empty() {
                println!(
                    "Found {} Blackwell devices in embedded database",
                    blackwell_devices.len()
                );
            }
        }

        #[test]
        fn test_real_pci_ids_file_if_available() {
            // Test with the embedded PCI IDs data - use this for consistent testing
            let result = get_pci_ids_database();
            assert!(
                result.is_ok(),
                "Should successfully read embedded PCI database"
            );

            let devices = result.unwrap();
            assert!(
                !devices.is_empty(),
                "Embedded PCI database should contain devices"
            );

            println!(
                "Loaded {} NVIDIA devices from embedded PCI database",
                devices.len()
            );

            // Print some sample devices for debugging
            for (id, name) in devices.iter().take(10) {
                println!("Device 0x{:04x}: {}", id, name);
            }

            // Look for some known Hopper devices that should be in the embedded file
            let hopper_devices: Vec<_> = devices
                .iter()
                .filter(|(_, name)| {
                    let name_lower = name.to_lowercase();
                    name_lower.contains("h100")
                        || name_lower.contains("gh100")
                        || name_lower.contains("h800")
                })
                .collect();

            println!("Found {} Hopper-related devices", hopper_devices.len());
            for (id, name) in &hopper_devices {
                println!("Hopper device 0x{:04x}: {}", id, name);
            }

            let blackwell_devices: Vec<_> = devices
                .iter()
                .filter(|(_, name)| {
                    let name_lower = name.to_lowercase();
                    name_lower.contains("b100")
                        || name_lower.contains("gb100")
                        || name_lower.contains("b200")
                })
                .collect();

            println!(
                "Found {} Blackwell-related devices",
                blackwell_devices.len()
            );
            for (id, name) in &blackwell_devices {
                println!("Blackwell device 0x{:04x}: {}", id, name);
            }

            // Just verify we have a reasonable number of devices
            assert!(
                devices.len() > 70,
                "Embedded PCI database should contain many devices (found {})",
                devices.len()
            );
        }

        #[test]
        fn test_architecture_detection_with_real_device_names() {
            // Test architecture detection with real device names from the PCI database

            // Real Hopper device names
            assert_eq!(
                classify_gpu_architecture("GH100 [H100 SXM5 80GB]"),
                GpuArchitecture::Hopper
            );
            assert_eq!(
                classify_gpu_architecture("GH100 [H100 PCIe]"),
                GpuArchitecture::Hopper
            );
            assert_eq!(
                classify_gpu_architecture("GH100 [H800 PCIe]"),
                GpuArchitecture::Hopper
            );
            assert_eq!(
                classify_gpu_architecture("GH100 [H200 SXM 141GB]"),
                GpuArchitecture::Hopper
            );

            // Real Blackwell device names
            assert_eq!(
                classify_gpu_architecture("GB100 [B200]"),
                GpuArchitecture::Blackwell
            );
            assert_eq!(
                classify_gpu_architecture("GB100 [TS4 / B100]"),
                GpuArchitecture::Blackwell
            );
            assert_eq!(
                classify_gpu_architecture("GB100 [HGX GB200]"),
                GpuArchitecture::Blackwell
            );

            // Edge cases with real naming patterns
            assert_eq!(classify_gpu_architecture("GH100"), GpuArchitecture::Hopper);
            assert_eq!(
                classify_gpu_architecture("GB100"),
                GpuArchitecture::Blackwell
            );
        }

        #[test]
        fn test_externalized_pci_ids_integration() {
            // Test the integration of PCI IDs functionality
            // This test demonstrates how the parsing functions work together

            // Create test data that includes subsystem entries (like the real file)
            let test_content = r#"
# Test PCI database with subsystem entries
10de  NVIDIA Corporation
	0020  NV4 [Riva TNT]
		1043 0200  V3400 TNT
		1048 0c18  Erazor II SGRAM
	2330  GH100 [H100 SXM5 80GB]
		10de 16c1  H100 SXM5 80GB
	2331  GH100 [H100 PCIe]
	2901  GB100 [B200]
		10de 1234  B200 Development Board
	2920  GB100 [TS4 / B100]
10df  Some Other Vendor
	1234  Other Device
"#;

            // Test the PCI database parsing function
            let result = parse_pci_database_content(test_content);
            assert!(
                result.is_ok(),
                "Should successfully parse PCI database with subsystems"
            );

            let devices = result.unwrap();

            // Verify we only get device entries, not subsystem entries
            assert_eq!(devices.len(), 5, "Should extract 5 NVIDIA device entries");

            // Verify specific devices (should have device entries but not subsystem entries)
            assert_eq!(devices.get(&0x0020), Some(&"NV4 [Riva TNT]".to_string()));
            assert_eq!(
                devices.get(&0x2330),
                Some(&"GH100 [H100 SXM5 80GB]".to_string())
            );
            assert_eq!(devices.get(&0x2331), Some(&"GH100 [H100 PCIe]".to_string()));
            assert_eq!(devices.get(&0x2901), Some(&"GB100 [B200]".to_string()));
            assert_eq!(
                devices.get(&0x2920),
                Some(&"GB100 [TS4 / B100]".to_string())
            );

            // Verify subsystem entries are not included
            assert_eq!(
                devices.get(&0x1043),
                None,
                "Subsystem vendor should not be included"
            );
            assert_eq!(
                devices.get(&0x16c1),
                None,
                "Subsystem device should not be included"
            );

            // Test architecture classification with the extracted names
            assert_eq!(
                classify_gpu_architecture(devices.get(&0x2330).unwrap()),
                GpuArchitecture::Hopper
            );
            assert_eq!(
                classify_gpu_architecture(devices.get(&0x2331).unwrap()),
                GpuArchitecture::Hopper
            );
            assert_eq!(
                classify_gpu_architecture(devices.get(&0x2901).unwrap()),
                GpuArchitecture::Blackwell
            );
            assert_eq!(
                classify_gpu_architecture(devices.get(&0x2920).unwrap()),
                GpuArchitecture::Blackwell
            );
            assert_eq!(
                classify_gpu_architecture(devices.get(&0x0020).unwrap()),
                GpuArchitecture::Unknown
            );

            println!("✓ PCI IDs functionality working correctly");
            println!("✓ Parsed {} NVIDIA devices from test data", devices.len());
            println!("✓ Correctly handled subsystem entries");
            println!("✓ Architecture detection working with real device names");
        }

        // Note: Testing get_gpu_architecture() requires actual GPU hardware or mock sysfs,
        // so it's not included in unit tests. Integration tests would be needed for that.
    }
}

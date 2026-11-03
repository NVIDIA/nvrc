// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! The CDI spec for the VM's ConnectX functions.
//!
//! kata runtime-rs annotates every ConnectX function it passes through as
//! `nvidia.com/nic=<index>`, indices in guest PCI order, and kata-agent
//! resolves those names against `/var/run/cdi` before it creates the
//! container, failing it after 100 s without a match. No other component
//! writes that spec yet. This module is the bridge until `nvidia-ctk cdi
//! generate` gains a NIC mode (NVIDIA/nvidia-container-toolkit#2197); then
//! the NIC spec comes the way the GPU spec does, one command and no code
//! here. The indices are per VM, as nvidia-ctk's GPU indices are, so the
//! contract holds when one container holds the kind's functions.

use std::fs;
use std::path::Path;

use log::info;
use pcilibs_rs::{catalog, Sysfs, INFINIBAND_DEV_DIR};

use crate::macros::ResultExt;
use crate::modprobe;

pub const DIR: &str = "/var/run/cdi";
pub const SPEC: &str = "/var/run/cdi/nvidia-nic.json";
pub const KIND: &str = "nvidia.com/nic";
const MELLANOX: u16 = 0x15b3;
/// PCI base class of network controllers: Ethernet (0x0200) and InfiniBand
/// (0x0207) functions alike, PF or VF.
const NETWORK: u32 = 0x02;

/// One device of the spec: a function and the RDMA character devices the
/// kernel registered for it, by name under [`INFINIBAND_DEV_DIR`].
pub struct Function {
    pub bdf: String,
    pub nodes: Vec<String>,
}

/// Write the spec when the VM holds ConnectX functions; a no-op otherwise.
/// Runs before the mode's services, so `/var/run/cdi` is created here once
/// and `nvidia-ctk` writes the GPU spec into it afterwards.
pub fn write_nic_spec() {
    let sysfs = Sysfs::default();
    let bdfs = connectx_functions(&sysfs);
    if bdfs.is_empty() {
        return;
    }
    // All three are in-tree and may be built in, leaving no module tree for
    // modprobe; their sysfs nodes show they are present. They register the
    // functions during probe, so the nodes exist once modprobe returns.
    // rdma_ucm provides /dev/infiniband/rdma_cm, which librdmacm opens; a
    // workload cannot load it after the module lockdown.
    modprobe::load_unless_present("mlx5_ib", "/sys/bus/auxiliary/drivers/mlx5_ib.rdma");
    modprobe::load_unless_present("ib_umad", "/sys/class/infiniband_mad");
    modprobe::load_unless_present("rdma_ucm", "/sys/class/misc/rdma_cm");
    let functions: Vec<Function> = bdfs
        .into_iter()
        .map(|bdf| Function {
            nodes: rdma_nodes(&sysfs, &bdf),
            bdf,
        })
        .collect();
    let spec = nic_spec(&functions, has_rdma_cm(&sysfs));
    fs::create_dir(DIR).or_panic(format_args!("create {DIR}"));
    fs::write(SPEC, spec).or_panic(format_args!("write {SPEC}"));
    info!("cdi: {SPEC}: {} {KIND} device(s)", functions.len());
}

/// The VM's ConnectX functions in PCI order, the order runtime-rs indexes
/// them in.
pub fn connectx_functions(sysfs: &Sysfs) -> Vec<String> {
    catalog::discover(sysfs, catalog::Catalog::builtin())
        .or_panic(format_args!("scan PCI functions"))
        .into_iter()
        .filter(|d| d.identity.vendor == MELLANOX && d.class >> 16 == NETWORK)
        .map(|d| d.bdf)
        .collect()
}

/// The spec text: one device per function, named by the function's index,
/// with its PCI address in the environment and its RDMA nodes. The address
/// is the one edit every device has, which the spec format requires; it also
/// tells the container which function device `<index>` is.
pub fn nic_spec(functions: &[Function], rdma_cm: bool) -> String {
    let devices: Vec<String> = functions
        .iter()
        .enumerate()
        .map(|(index, function)| {
            let mut nodes = function.nodes.clone();
            if rdma_cm {
                nodes.push("rdma_cm".to_owned());
            }
            nodes.sort();
            let mut edits = vec![format!(
                "\"env\": [\"NVIDIA_NIC_{index}={}\"]",
                function.bdf
            )];
            if !nodes.is_empty() {
                let nodes: Vec<String> = nodes
                    .iter()
                    .map(|node| format!("{{\"path\": \"{INFINIBAND_DEV_DIR}/{node}\"}}"))
                    .collect();
                edits.push(format!("\"deviceNodes\": [{}]", nodes.join(", ")));
            }
            format!(
                "    {{\"name\": \"{index}\", \"containerEdits\": {{{}}}}}",
                edits.join(", ")
            )
        })
        .collect();
    format!(
        "{{\n  \"cdiVersion\": \"0.6.0\",\n  \"kind\": \"{KIND}\",\n  \"devices\": [\n{}\n  ]\n}}\n",
        devices.join(",\n")
    )
}

/// The `uverbs<N>`, `umad<N>` and `issm<N>` devices registered for the
/// function's InfiniBand devices, sorted. Empty for a function the RDMA
/// driver has not registered, an Ethernet function with RDMA off.
fn rdma_nodes(sysfs: &Sysfs, bdf: &str) -> Vec<String> {
    let ibdevs = names(&sysfs.infiniband(), |path| {
        fs::read_link(path.join("device"))
            .ok()
            .and_then(|target| target.file_name().map(|name| name == bdf))
            .unwrap_or(false)
    });
    let mad = sysfs.infiniband().with_file_name("infiniband_mad");
    let mut nodes: Vec<String> = [sysfs.infiniband_verbs(), mad]
        .iter()
        .flat_map(|class| {
            names(class, |path| {
                fs::read_to_string(path.join("ibdev"))
                    .map(|text| ibdevs.iter().any(|ibdev| ibdev == text.trim()))
                    .unwrap_or(false)
            })
        })
        .collect();
    nodes.sort();
    nodes
}

/// Whether the kernel registered the RDMA connection manager, the misc
/// device librdmacm opens; absent on a kernel built without rdma_ucm.
fn has_rdma_cm(sysfs: &Sysfs) -> bool {
    fs::metadata(sysfs.infiniband().with_file_name("misc").join("rdma_cm")).is_ok()
}

/// The entry names of a sysfs class directory whose entries pass `keep`.
fn names(class: &Path, keep: impl Fn(&Path) -> bool) -> Vec<String> {
    fs::read_dir(class)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| keep(path))
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcilibs_rs::testfs::{fake, Fake};

    fn function(f: &Fake, bdf: &str, vendor: u16, class: u32) {
        f.add_pci_device(bdf, vendor, 0x1021, class, None);
        // discover() reads the whole identity; the fake writes the rest.
        fs::write(f.device(bdf).join("subsystem_device"), "0x0000\n").unwrap();
    }

    fn class_entry(f: &Fake, class: &str, name: &str, ibdev: &str) {
        let path = f.sysfs.infiniband().with_file_name(class).join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("ibdev"), format!("{ibdev}\n")).unwrap();
    }

    fn nic(bdf: &str, nodes: &[&str]) -> Function {
        Function {
            bdf: bdf.to_owned(),
            nodes: nodes.iter().map(|node| (*node).to_owned()).collect(),
        }
    }

    #[test]
    fn connectx_functions_are_mellanox_network_functions_in_pci_order() {
        let f = fake();
        function(&f, "0000:05:00.0", MELLANOX, 0x020000); // Ethernet VF
        function(&f, "0000:01:00.0", MELLANOX, 0x020700); // InfiniBand PF
        function(&f, "0000:03:00.0", 0x8086, 0x020000); // another vendor's NIC
        function(&f, "0000:04:00.0", MELLANOX, 0x060400); // a Mellanox bridge
        assert_eq!(
            connectx_functions(&f.sysfs),
            vec!["0000:01:00.0".to_owned(), "0000:05:00.0".to_owned()]
        );
    }

    #[test]
    fn rdma_nodes_are_the_functions_own_verbs_and_mad_devices() {
        let f = fake();
        f.add_infiniband("mlx5_0", "0000:01:00.0", "1: CA", "");
        f.add_infiniband_verbs("uverbs0", "mlx5_0", "231:192");
        class_entry(&f, "infiniband_mad", "umad0", "mlx5_0");
        class_entry(&f, "infiniband_mad", "issm0", "mlx5_0");
        f.add_infiniband("mlx5_1", "0000:05:00.0", "1: CA", "");
        f.add_infiniband_verbs("uverbs1", "mlx5_1", "231:193");
        assert_eq!(
            rdma_nodes(&f.sysfs, "0000:01:00.0"),
            vec!["issm0".to_owned(), "umad0".to_owned(), "uverbs0".to_owned()]
        );
        assert_eq!(
            rdma_nodes(&f.sysfs, "0000:05:00.0"),
            vec!["uverbs1".to_owned()]
        );
        assert!(rdma_nodes(&f.sysfs, "0000:07:00.0").is_empty());
    }

    #[test]
    fn rdma_cm_is_a_misc_device() {
        let f = fake();
        assert!(!has_rdma_cm(&f.sysfs));
        fs::create_dir_all(f.sysfs.infiniband().with_file_name("misc").join("rdma_cm")).unwrap();
        assert!(has_rdma_cm(&f.sysfs));
    }

    #[test]
    fn nic_spec_names_functions_by_index_with_their_nodes() {
        let functions = [
            nic("0000:01:00.0", &["uverbs0", "umad0", "issm0"]),
            nic("0000:05:00.0", &["uverbs1"]),
        ];
        let expected = [
            "{",
            "  \"cdiVersion\": \"0.6.0\",",
            "  \"kind\": \"nvidia.com/nic\",",
            "  \"devices\": [",
            "    {\"name\": \"0\", \"containerEdits\": {\"env\": [\"NVIDIA_NIC_0=0000:01:00.0\"], \"deviceNodes\": [{\"path\": \"/dev/infiniband/issm0\"}, {\"path\": \"/dev/infiniband/rdma_cm\"}, {\"path\": \"/dev/infiniband/umad0\"}, {\"path\": \"/dev/infiniband/uverbs0\"}]}},",
            "    {\"name\": \"1\", \"containerEdits\": {\"env\": [\"NVIDIA_NIC_1=0000:05:00.0\"], \"deviceNodes\": [{\"path\": \"/dev/infiniband/rdma_cm\"}, {\"path\": \"/dev/infiniband/uverbs1\"}]}}",
            "  ]",
            "}",
            "",
        ]
        .join("\n");
        assert_eq!(nic_spec(&functions, true), expected);
    }

    #[test]
    fn a_function_without_rdma_still_gets_its_device() {
        let expected = [
            "{",
            "  \"cdiVersion\": \"0.6.0\",",
            "  \"kind\": \"nvidia.com/nic\",",
            "  \"devices\": [",
            "    {\"name\": \"0\", \"containerEdits\": {\"env\": [\"NVIDIA_NIC_0=0000:02:00.0\"]}}",
            "  ]",
            "}",
            "",
        ]
        .join("\n");
        assert_eq!(nic_spec(&[nic("0000:02:00.0", &[])], false), expected);
    }
}

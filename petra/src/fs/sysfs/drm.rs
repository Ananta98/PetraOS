// SPDX-License-Identifier: GPL-2.0

//! SysFS DRM/KMS Class Subsystem (`/sys/class/drm`).
//!
//! Exposes DRM card devices, render nodes, and display connectors to userspace
//! compositors (KWin Wayland, Weston) and graphics stacks (Mesa, libdrm).

use alloc::{format, string::String, sync::Arc, vec, vec::Vec};

use crate::{
    drivers::drm::core as drm_core,
    fs::sysfs::kobject::{
        SysAttrNode, SysNode, SysStaticAttrNode, SysSymlinkNode, alloc_sysfs_ino,
    },
    uapi::drm,
};

const DRM_MAJOR: u32 = 226;

/// Maps a DRM connector type code to standard Linux connector name prefix.
pub fn connector_type_prefix(ty: u32) -> &'static str {
    match ty {
        drm::DRM_MODE_CONNECTOR_HDMIA => "HDMI-A",
        drm::DRM_MODE_CONNECTOR_DISPLAYPORT => "DP",
        drm::DRM_MODE_CONNECTOR_EDP => "eDP",
        drm::DRM_MODE_CONNECTOR_VIRTUAL => "Virtual",
        _ => "Virtual",
    }
}

/// Directory `/sys/class/drm`.
pub struct SysClassDrmDirNode {
    ino: usize,
}

impl SysClassDrmDirNode {
    /// Creates a new `/sys/class/drm` directory node.
    pub fn new() -> Self {
        Self {
            ino: alloc_sysfs_ino(),
        }
    }
}

impl Default for SysClassDrmDirNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SysNode for SysClassDrmDirNode {
    fn name(&self) -> &str {
        "drm"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        if let Some(card_idx) = name.strip_prefix("card") {
            if let Some((idx_s, conn_s)) = card_idx.split_once('-') {
                let minor: u32 = idx_s.parse().ok()?;
                let dev = drm_core::get_device(minor).ok()?;
                let kms = dev.kms()?;
                for p in 0..kms.pipeline_count() {
                    let (conn_ty, _, _, _) = kms.pipeline_connector(p);
                    let prefix = connector_type_prefix(conn_ty);
                    let expected_conn = format!("{prefix}-{}", p + 1);
                    if conn_s == expected_conn {
                        return Some(Arc::new(SysDrmConnectorDirNode::new(
                            minor,
                            name,
                            p,
                            dev,
                        )));
                    }
                }
                return None;
            }

            let minor: u32 = card_idx.parse().ok()?;
            let dev = drm_core::get_device(minor).ok()?;
            return Some(Arc::new(SysDrmCardDirNode::new(minor, dev)));
        }

        if let Some(render_idx) = name.strip_prefix("renderD") {
            let rminor: u32 = render_idx.parse().ok()?;
            let minor = rminor.saturating_sub(128);
            let dev = drm_core::get_device(minor).ok()?;
            return Some(Arc::new(SysDrmRenderDirNode::new(rminor, dev)));
        }

        None
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        let mut children = Vec::new();
        let devices = drm_core::DRM_DEVICES.lock();

        for (minor_idx, dev) in devices.iter().enumerate() {
            let minor = minor_idx as u32;
            children.push((format!("card{minor}"), true));
            children.push((format!("renderD{}", 128 + minor), true));

            if let Some(kms) = dev.kms() {
                for p in 0..kms.pipeline_count() {
                    let (conn_ty, _, _, _) = kms.pipeline_connector(p);
                    let prefix = connector_type_prefix(conn_ty);
                    let conn_name = format!("card{minor}-{prefix}-{}", p + 1);
                    children.push((conn_name, true));
                }
            }
        }

        children
    }
}

/// Directory `/sys/class/drm/cardN`.
pub struct SysDrmCardDirNode {
    minor: u32,
    dev: Arc<dyn drm_core::DrmDevice>,
    ino: usize,
}

impl SysDrmCardDirNode {
    /// Creates a new DRM card device node.
    pub fn new(minor: u32, dev: Arc<dyn drm_core::DrmDevice>) -> Self {
        Self {
            minor,
            dev,
            ino: alloc_sysfs_ino(),
        }
    }
}

impl SysNode for SysDrmCardDirNode {
    fn name(&self) -> &str {
        "card"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        let minor = self.minor;
        match name {
            "dev" => Some(Arc::new(SysAttrNode::new("dev", move || {
                format!("{DRM_MAJOR}:{minor}\n").into_bytes()
            }))),
            "uevent" => Some(Arc::new(SysAttrNode::new("uevent", move || {
                format!("MAJOR={DRM_MAJOR}\nMINOR={minor}\nDEVNAME=dri/card{minor}\nDEVTYPE=drm_minor\n")
                    .into_bytes()
            }))),
            "subsystem" => Some(Arc::new(SysSymlinkNode::new("subsystem", "../../class/drm"))),
            "device" => Some(Arc::new(SysSymlinkNode::new(
                "device",
                "../../devices/virtual/drm",
            ))),
            _ => {
                let kms = self.dev.kms()?;
                for p in 0..kms.pipeline_count() {
                    let (conn_ty, _, _, _) = kms.pipeline_connector(p);
                    let prefix = connector_type_prefix(conn_ty);
                    let expected_name = format!("card{minor}-{prefix}-{}", p + 1);
                    if name == expected_name {
                        return Some(Arc::new(SysDrmConnectorDirNode::new(
                            minor,
                            name,
                            p,
                            Arc::clone(&self.dev),
                        )));
                    }
                }
                None
            }
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        let mut children = vec![
            (String::from("dev"), false),
            (String::from("uevent"), false),
            (String::from("subsystem"), false),
            (String::from("device"), false),
        ];

        let minor = self.minor;
        if let Some(kms) = self.dev.kms() {
            for p in 0..kms.pipeline_count() {
                let (conn_ty, _, _, _) = kms.pipeline_connector(p);
                let prefix = connector_type_prefix(conn_ty);
                children.push((format!("card{minor}-{prefix}-{}", p + 1), true));
            }
        }

        children
    }
}

/// Directory for a KMS connector (`/sys/class/drm/cardN-<type>-<idx>`).
pub struct SysDrmConnectorDirNode {
    minor: u32,
    name: String,
    pipeline: usize,
    dev: Arc<dyn drm_core::DrmDevice>,
    ino: usize,
}

impl SysDrmConnectorDirNode {
    /// Creates a new DRM connector node.
    pub fn new(
        minor: u32,
        name: &str,
        pipeline: usize,
        dev: Arc<dyn drm_core::DrmDevice>,
    ) -> Self {
        Self {
            minor,
            name: String::from(name),
            pipeline,
            dev,
            ino: alloc_sysfs_ino(),
        }
    }
}

impl SysNode for SysDrmConnectorDirNode {
    fn name(&self) -> &str {
        &self.name
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        let minor = self.minor;
        let pipeline = self.pipeline;
        let dev = Arc::clone(&self.dev);

        match name {
            "status" => Some(Arc::new(SysStaticAttrNode::new("status", b"connected\n"))),
            "enabled" => Some(Arc::new(SysStaticAttrNode::new("enabled", b"enabled\n"))),
            "dpms" => Some(Arc::new(SysStaticAttrNode::new("dpms", b"On\n"))),
            "edid" => Some(Arc::new(SysStaticAttrNode::new("edid", b""))),
            "modes" => Some(Arc::new(SysAttrNode::new("modes", move || {
                let mut out = String::new();
                if let Some(kms) = dev.kms() {
                    for mode in kms.pipeline_modes(pipeline) {
                        out.push_str(&format!("{}x{}\n", mode.hdisplay, mode.vdisplay));
                    }
                }
                if out.is_empty() {
                    out.push_str("1920x1080\n1024x768\n800x600\n");
                }
                out.into_bytes()
            }))),
            "uevent" => Some(Arc::new(SysAttrNode::new("uevent", move || {
                format!("MAJOR={DRM_MAJOR}\nMINOR={minor}\nDEVTYPE=drm_connector\n").into_bytes()
            }))),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![
            (String::from("status"), false),
            (String::from("enabled"), false),
            (String::from("dpms"), false),
            (String::from("modes"), false),
            (String::from("edid"), false),
            (String::from("uevent"), false),
        ]
    }
}

/// Directory `/sys/class/drm/renderDN`.
pub struct SysDrmRenderDirNode {
    rminor: u32,
    _dev: Arc<dyn drm_core::DrmDevice>,
    ino: usize,
}

impl SysDrmRenderDirNode {
    /// Creates a new DRM render device node.
    pub fn new(rminor: u32, dev: Arc<dyn drm_core::DrmDevice>) -> Self {
        Self {
            rminor,
            _dev: dev,
            ino: alloc_sysfs_ino(),
        }
    }
}

impl SysNode for SysDrmRenderDirNode {
    fn name(&self) -> &str {
        "render"
    }

    fn ino(&self) -> usize {
        self.ino
    }

    fn is_dir(&self) -> bool {
        true
    }

    fn get_child(&self, name: &str) -> Option<Arc<dyn SysNode>> {
        let rminor = self.rminor;
        match name {
            "dev" => Some(Arc::new(SysAttrNode::new("dev", move || {
                format!("{DRM_MAJOR}:{rminor}\n").into_bytes()
            }))),
            "uevent" => Some(Arc::new(SysAttrNode::new("uevent", move || {
                format!("MAJOR={DRM_MAJOR}\nMINOR={rminor}\nDEVNAME=dri/renderD{rminor}\nDEVTYPE=drm_minor\n")
                    .into_bytes()
            }))),
            "subsystem" => Some(Arc::new(SysSymlinkNode::new("subsystem", "../../class/drm"))),
            "device" => Some(Arc::new(SysSymlinkNode::new(
                "device",
                "../../devices/virtual/drm",
            ))),
            _ => None,
        }
    }

    fn list_children(&self) -> Vec<(String, bool)> {
        vec![
            (String::from("dev"), false),
            (String::from("uevent"), false),
            (String::from("subsystem"), false),
            (String::from("device"), false),
        ]
    }
}

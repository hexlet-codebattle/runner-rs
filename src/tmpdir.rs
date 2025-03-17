use std::path::PathBuf;

use uuid::Uuid;

/// Used to prepare a chroot jail by mounting / somewhere inside /tmp as overlay
pub struct TmpDir {
    base: PathBuf,
    chroot: PathBuf,
}

impl TmpDir {
    /// Creates new directory in /tmp,
    /// populates it with dirs required for overlayfs,
    /// mounts current root there as an overlay.
    pub fn new() -> std::io::Result<Self> {
        let mut path = PathBuf::from(String::from("/tmp"));
        path.push(Uuid::new_v4().to_string());
        std::fs::create_dir(&path)?;
        // Create dirs in tmp to mount overlay
        let upper = path.join("upper");
        std::fs::create_dir(&upper)?;
        let work = path.join("work");
        std::fs::create_dir(&work)?;
        let merged = path.join("merged");
        std::fs::create_dir(&merged)?;

        let this = Self {
            base: path,
            chroot: merged.clone(),
        };

        // Mount root as overlay
        #[allow(unused)]
        let opts = format!(
            "lowerdir=/,upperdir={},workdir={}",
            upper.display(),
            work.display()
        );

        // Only Linux has overlayfs, and this code is supposed
        // to work only in container environment.
        // Condition here exists only for the purpose of muting errors
        // on other systems during development.
        #[cfg(target_os = "linux")]
        {
            use nix::{mount, sys::stat};
            mount::mount(
                Some("overlay"),
                &merged,
                Some("overlay"),
                mount::MsFlags::empty(),
                Some(opts.as_str()),
            )?;
            mount::mount(
                Some("proc"),
                &merged.join("proc"),
                Some("proc"),
                mount::MsFlags::empty(),
                Some(""),
            )?;
            let mode = stat::Mode::from_bits(0o666).unwrap();
            // For Go
            stat::mknod(
                &merged.join("dev/null"),
                stat::SFlag::S_IFCHR,
                mode,
                stat::makedev(1, 3),
            )?;
            // For C#
            stat::mknod(
                &merged.join("dev/urandom"),
                stat::SFlag::S_IFCHR,
                mode,
                stat::makedev(1, 9),
            )?;
        }
        Ok(this)
    }

    /// Returns the directory prepared for chroot
    pub fn chroot(&self) -> &PathBuf {
        &self.chroot
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        // See the new() function for explanation
        #[cfg(target_os = "linux")]
        {
            if let Err(e) = nix::mount::umount(&self.chroot.join("proc")) {
                log::error!("umount proc: {}", e);
            }
            if let Err(e) = nix::mount::umount(&self.chroot) {
                log::error!("umount chroot overlay: {}", e);
            }
        }
        if let Err(e) = std::fs::remove_dir_all(&self.base) {
            log::error!("remove tmp dir: {}", e);
        }
    }
}

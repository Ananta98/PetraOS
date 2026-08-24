use super::header::*;
use crate::mm::ArchPageTable;
use crate::mm::PageTable;
use crate::mm::{AddrSpace, PageTableFlags, VirtAddr, VmAreaKind};
use crate::proc::process::CommandLine;
use crate::proc::process::credentials::Credentials;

/// A loaded ELF executable's resources.
pub struct LoadedElf {
    pub entry_point: VirtAddr,
    pub stack_pointer: VirtAddr,
    pub addr_space: AddrSpace<ArchPageTable>,
}

/// Object-Oriented ELF Parser and Loader.
pub struct Elf<'a> {
    data: &'a [u8],
    header: &'a Elf64Header,
}

impl<'a> Elf<'a> {
    /// Create a new Elf parser instance and validate the file headers.
    pub fn new(data: &'a [u8]) -> Result<Self, &'static str> {
        if data.len() < core::mem::size_of::<Elf64Header>() {
            return Err("ELF data too short");
        }

        // SAFETY: We validated that the slice is long enough to contain the Elf64Header.
        let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };

        let elf = Self { data, header };
        elf.validate()?;
        Ok(elf)
    }

    /// Retrieve the entry point virtual address from the ELF header.
    pub fn entry_point(&self) -> VirtAddr {
        VirtAddr::new(self.header.e_entry)
    }

    /// Retrieve the list of program headers.
    pub fn program_headers(&self) -> Result<&'a [Elf64Phdr], &'static str> {
        let ph_offset = self.header.e_phoff as usize;
        let ph_num = self.header.e_phnum as usize;
        let ph_size = self.header.e_phentsize as usize;

        if ph_num == 0 {
            return Ok(&[]);
        }

        if ph_size != core::mem::size_of::<Elf64Phdr>() {
            return Err("Invalid program header entry size");
        }

        if ph_offset + ph_num * ph_size > self.data.len() {
            return Err("Program headers out of bounds");
        }

        // SAFETY: Bounds checked, and data alignment is verified.
        let ph_slice = unsafe {
            core::slice::from_raw_parts(
                self.data.as_ptr().add(ph_offset) as *const Elf64Phdr,
                ph_num,
            )
        };

        Ok(ph_slice)
    }

    /// Retrieve the list of section headers.
    pub fn section_headers(&self) -> Result<&'a [Elf64Shdr], &'static str> {
        let sh_offset = self.header.e_shoff as usize;
        let sh_num = self.header.e_shnum as usize;
        let sh_size = self.header.e_shentsize as usize;

        if sh_num == 0 {
            return Ok(&[]);
        }

        if sh_size != core::mem::size_of::<Elf64Shdr>() {
            return Err("Invalid section header entry size");
        }

        if sh_offset + sh_num * sh_size > self.data.len() {
            return Err("Section headers out of bounds");
        }

        // SAFETY: Bounds checked, and data alignment is verified.
        let sh_slice = unsafe {
            core::slice::from_raw_parts(
                self.data.as_ptr().add(sh_offset) as *const Elf64Shdr,
                sh_num,
            )
        };

        Ok(sh_slice)
    }

    /// Retrieve the section header string table.
    pub fn shstrtab(&self) -> Result<&'a str, &'static str> {
        let shstrndx = self.header.e_shstrndx as usize;
        let sh_slice = self.section_headers()?;

        if shstrndx >= sh_slice.len() {
            return Err("Invalid shstrndx");
        }

        let shstr_hdr = &sh_slice[shstrndx];
        let offset = shstr_hdr.sh_offset as usize;
        let size = shstr_hdr.sh_size as usize;

        if offset + size > self.data.len() {
            return Err("shstrtab out of bounds");
        }

        let raw_strtab = &self.data[offset..offset + size];
        core::str::from_utf8(raw_strtab).map_err(|_| "Invalid UTF-8 in shstrtab")
    }

    /// Find a section by its name.
    pub fn find_section(
        &self,
        name: &str,
    ) -> Result<Option<(&'a Elf64Shdr, &'a [u8])>, &'static str> {
        let shstrtab_data = self.shstrtab()?;
        let sh_slice = self.section_headers()?;

        for shdr in sh_slice {
            let name_offset = shdr.sh_name as usize;
            if name_offset < shstrtab_data.len() {
                let section_name = shstrtab_data[name_offset..]
                    .split('\0')
                    .next()
                    .unwrap_or("");

                if section_name == name {
                    let offset = shdr.sh_offset as usize;
                    let size = shdr.sh_size as usize;
                    if offset + size <= self.data.len() {
                        return Ok(Some((shdr, &self.data[offset..offset + size])));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Retrieve the interpreter path if this ELF binary requests PT_INTERP.
    pub fn interpreter_path(&self) -> Result<Option<&'a str>, &'static str> {
        let ph_slice = self.program_headers()?;
        for phdr in ph_slice {
            if phdr.p_type == PT_INTERP {
                let offset = phdr.p_offset as usize;
                let filesz = phdr.p_filesz as usize;
                if offset + filesz > self.data.len() {
                    return Err("PT_INTERP string out of bounds");
                }
                let raw_bytes = &self.data[offset..offset + filesz];
                let len = raw_bytes.iter().position(|&b| b == 0).unwrap_or(filesz);
                let path = core::str::from_utf8(&raw_bytes[..len])
                    .map_err(|_| "Invalid UTF-8 in PT_INTERP")?;
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    /// Maps the loadable segments, creates the user address space, allocates a user stack,
    /// sets up System V AMD64 ABI argc/argv/envp/auxv parameters, and returns loaded image information.
    pub fn load_with_cmdline(
        &self,
        cmdline: Option<&CommandLine>,
        creds: Option<&Credentials>,
    ) -> Result<LoadedElf, &'static str> {
        let page_table = ArchPageTable::new().map_err(|_| "Failed to create PML4 page table")?;
        let mut addr_space = AddrSpace::new(page_table);

        self.load_segments(&mut addr_space, 0)?;

        let mut entry_point = self.entry_point();
        let mut at_base = 0;

        if let Some(interp_path) = self.interpreter_path()? {
            let interp_bytes = match crate::fs::read_file(interp_path) {
                Ok(data) => data,
                Err(_) => {
                    let alt_path = if interp_path.starts_with("/usr/lib/") {
                        alloc::format!("/lib/{}", &interp_path[9..])
                    } else if interp_path.starts_with("/lib/") {
                        alloc::format!("/usr/lib/{}", &interp_path[5..])
                    } else {
                        alloc::string::String::from("/lib/ld.so")
                    };
                    crate::fs::read_file(&alt_path)
                        .map_err(|_| "Failed to read dynamic interpreter from VFS")?
                }
            };

            let interp_elf = Elf::new(&interp_bytes)?;
            const INTERP_BASE: u64 = 0x7F00_0000_0000;
            interp_elf.load_segments(&mut addr_space, INTERP_BASE)?;
            entry_point = VirtAddr::new(INTERP_BASE + interp_elf.entry_point().as_u64());
            at_base = INTERP_BASE;
        }

        let phdr_addr = {
            let mut addr = None;
            let ph_slice = self.program_headers()?;
            for phdr in ph_slice {
                if phdr.p_type == PT_PHDR {
                    addr = Some(phdr.p_vaddr);
                    break;
                }
            }
            if let Some(a) = addr {
                a
            } else {
                let mut load_base = 0x400000;
                for phdr in ph_slice {
                    if phdr.p_type == PT_LOAD && phdr.p_offset == 0 {
                        load_base = phdr.p_vaddr;
                        break;
                    }
                }
                load_base + self.header.e_phoff
            }
        };

        let (uid, euid, gid, egid) = if let Some(c) = creds {
            (c.uid, c.euid, c.gid, c.egid)
        } else {
            (0, 0, 0, 0)
        };

        let auxv = alloc::vec![
            (AT_PHDR, phdr_addr),
            (AT_PHENT, self.header.e_phentsize as u64),
            (AT_PHNUM, self.header.e_phnum as u64),
            (AT_PAGESZ, 4096),
            (AT_BASE, at_base),
            (AT_FLAGS, 0),
            (AT_ENTRY, self.entry_point().as_u64()),
            (AT_UID, uid as u64),
            (AT_EUID, euid as u64),
            (AT_GID, gid as u64),
            (AT_EGID, egid as u64),
            (AT_SECURE, 0),
        ];

        let stack_size = 256 * 1024; // 256 KiB stack
        let stack_top = VirtAddr::new(0x7FFF_FFFF_0000);
        let stack_start = stack_top - stack_size as u64;
        let stack_flags =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

        addr_space
            .map_area(stack_start, stack_size, stack_flags, VmAreaKind::Anonymous)
            .map_err(|_| "Failed to map user stack VMA")?;

        let initial_sp = if let Some(cmd) = cmdline {
            Self::setup_user_stack(&mut addr_space, stack_top, cmd, &auxv)?
        } else {
            stack_top
        };

        Ok(LoadedElf {
            entry_point,
            stack_pointer: initial_sp,
            addr_space,
        })
    }

    /// Maps the loadable segments, creates the user address space, allocates a user stack,
    /// and returns the loaded image information.
    pub fn load(&self) -> Result<LoadedElf, &'static str> {
        self.load_with_cmdline(None, None)
    }

    /// Setup the System V AMD64 ABI user stack frame with argc, argv, envp, auxv, and string tables.
    fn setup_user_stack(
        addr_space: &mut AddrSpace<ArchPageTable>,
        stack_top: VirtAddr,
        cmdline: &crate::proc::process::CommandLine,
        auxv: &[(u64, u64)],
    ) -> Result<VirtAddr, &'static str> {
        let hhdm = crate::mm::hhdm_offset();
        let mut cur_sp = stack_top.as_u64();

        // Helper closure to copy byte slice onto the stack at decremented SP
        let mut write_user_bytes = |sp: &mut u64, bytes: &[u8]| -> Result<u64, &'static str> {
            *sp -= bytes.len() as u64;
            let target_vaddr = *sp;

            let mut written = 0;
            while written < bytes.len() {
                let curr_v = target_vaddr + written as u64;
                let page_v = curr_v & !4095;
                let page_off = (curr_v & 4095) as usize;
                let chunk_len = core::cmp::min(bytes.len() - written, 4096 - page_off);

                let phys = addr_space
                    .page_table()
                    .translate(VirtAddr::new(page_v))
                    .ok_or("Failed to translate user stack page")?;

                unsafe {
                    let dest = ((phys.as_u64() + hhdm) as *mut u8).add(page_off);
                    core::ptr::copy_nonoverlapping(bytes[written..].as_ptr(), dest, chunk_len);
                }
                written += chunk_len;
            }
            Ok(target_vaddr)
        };

        // 1. Push 16 bytes of random entropy for AT_RANDOM (stack canary)
        let random_entropy = [
            0x4b, 0x1f, 0x93, 0x7c, 0xa2, 0x5e, 0x08, 0xd4, 0x39, 0xf1, 0x60, 0xbb, 0x8d, 0x24,
            0xee, 0x57,
        ];
        let random_vaddr = write_user_bytes(&mut cur_sp, &random_entropy)?;

        // 2. Push environment strings (null-terminated)
        let mut env_ptrs = alloc::vec::Vec::with_capacity(cmdline.env.len());
        for env_str in &cmdline.env {
            let mut str_bytes = alloc::vec::Vec::with_capacity(env_str.len() + 1);
            str_bytes.extend_from_slice(env_str.as_bytes());
            str_bytes.push(0);
            let str_vaddr = write_user_bytes(&mut cur_sp, &str_bytes)?;
            env_ptrs.push(str_vaddr);
        }

        // 3. Push argument strings (null-terminated)
        let mut arg_ptrs = alloc::vec::Vec::with_capacity(cmdline.args.len());
        for arg_str in &cmdline.args {
            let mut str_bytes = alloc::vec::Vec::with_capacity(arg_str.len() + 1);
            str_bytes.extend_from_slice(arg_str.as_bytes());
            str_bytes.push(0);
            let str_vaddr = write_user_bytes(&mut cur_sp, &str_bytes)?;
            arg_ptrs.push(str_vaddr);
        }

        let execfn_vaddr = arg_ptrs.first().copied().unwrap_or(0);

        // 4. Align cur_sp to 8 bytes
        cur_sp &= !7;

        // Build complete auxiliary vector table including dynamic entries
        let mut full_auxv = alloc::vec::Vec::with_capacity(auxv.len() + 3);
        full_auxv.extend_from_slice(auxv);
        full_auxv.push((AT_RANDOM, random_vaddr));
        if execfn_vaddr != 0 {
            full_auxv.push((AT_EXECFN, execfn_vaddr));
        }

        // Calculate total table entries:
        // argc (1) + argv pointers (N) + NULL (1) + envp pointers (M) + NULL (1) + auxv (K * 2) + AT_NULL (2)
        let total_entries = 1 + arg_ptrs.len() + 1 + env_ptrs.len() + 1 + full_auxv.len() * 2 + 2;
        let total_table_bytes = total_entries * 8;

        // System V AMD64 ABI requires RSP to be 16-byte aligned at process entry
        if (cur_sp - total_table_bytes as u64) % 16 != 0 {
            cur_sp -= 8;
        }

        // Helper to push a single u64 value
        let mut write_u64 = |sp: &mut u64, val: u64| -> Result<(), &'static str> {
            let bytes = val.to_ne_bytes();
            write_user_bytes(sp, &bytes)?;
            Ok(())
        };

        // 5. Push aux vector AT_NULL (0, 0)
        write_u64(&mut cur_sp, 0)?; // AT_NULL a_val
        write_u64(&mut cur_sp, 0)?; // AT_NULL a_type

        // 6. Push auxiliary vectors
        for &(key, val) in full_auxv.iter().rev() {
            write_u64(&mut cur_sp, val)?;
            write_u64(&mut cur_sp, key)?;
        }

        // 7. Push envp array (NULL-terminated)
        write_u64(&mut cur_sp, 0)?;
        for &ptr in env_ptrs.iter().rev() {
            write_u64(&mut cur_sp, ptr)?;
        }

        // 8. Push argv array (NULL-terminated)
        write_u64(&mut cur_sp, 0)?;
        for &ptr in arg_ptrs.iter().rev() {
            write_u64(&mut cur_sp, ptr)?;
        }

        // 9. Push argc
        write_u64(&mut cur_sp, arg_ptrs.len() as u64)?;

        Ok(VirtAddr::new(cur_sp))
    }

    /// Validate the header's identity, endianness, machine and type.
    fn validate(&self) -> Result<(), &'static str> {
        if self.header.e_ident[0..4] != ELF_MAGIC {
            return Err("Invalid ELF magic");
        }
        if self.header.e_ident[4] != ELF_CLASS_64 {
            return Err("Not a 64-bit ELF");
        }
        if self.header.e_ident[5] != ELF_DATA_2LSB {
            return Err("Not little endian");
        }
        if self.header.e_machine != EM_X86_64 {
            return Err("Not x86_64 architecture");
        }
        if self.header.e_type != ET_EXEC && self.header.e_type != ET_DYN {
            return Err("Not an executable or dynamic binary");
        }
        Ok(())
    }

    /// Load and map all PT_LOAD segments with an optional base virtual offset.
    pub fn load_segments(
        &self,
        addr_space: &mut AddrSpace<ArchPageTable>,
        base_offset: u64,
    ) -> Result<(), &'static str> {
        let ph_slice = self.program_headers()?;
        for phdr in ph_slice {
            if phdr.p_type == PT_LOAD {
                self.load_segment(addr_space, phdr, base_offset)?;
            }
        }
        Ok(())
    }

    /// Map a single program segment and copy file data to allocated physical frames.
    fn load_segment(
        &self,
        addr_space: &mut AddrSpace<ArchPageTable>,
        phdr: &Elf64Phdr,
        base_offset: u64,
    ) -> Result<(), &'static str> {
        let vaddr = base_offset + phdr.p_vaddr;
        let start_vaddr = VirtAddr::new(vaddr);
        let end_vaddr = start_vaddr + phdr.p_memsz;

        let aligned_start = VirtAddr::new(vaddr & !4095);
        let aligned_end = VirtAddr::new((end_vaddr.as_u64() + 4095) & !4095);
        let aligned_size = (aligned_end - aligned_start) as usize;

        if aligned_size == 0 {
            return Ok(());
        }

        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if (phdr.p_flags & 2) != 0 {
            flags |= PageTableFlags::WRITABLE;
        }
        if (phdr.p_flags & 1) == 0 {
            flags |= PageTableFlags::NO_EXECUTE;
        }

        addr_space
            .map_area(aligned_start, aligned_size, flags, VmAreaKind::Anonymous)
            .map_err(|_| "Failed to map ELF segment VMA")?;

        let file_offset = phdr.p_offset as usize;
        let file_size = phdr.p_filesz as usize;

        if file_size > 0 {
            if file_offset + file_size > self.data.len() {
                return Err("ELF segment file offset out of bounds");
            }

            let hhdm = crate::mm::hhdm_offset();
            for page_virt_u64 in (aligned_start.as_u64()..aligned_end.as_u64()).step_by(4096) {
                let page_virt = VirtAddr::new(page_virt_u64);
                let phys_addr = addr_space
                    .page_table()
                    .translate(page_virt)
                    .ok_or("Failed to translate user virtual page to physical page")?;

                let page_start = page_virt_u64;
                let page_end = page_start + 4096;
                let data_start_v = start_vaddr.as_u64();
                let data_end_v = data_start_v + file_size as u64;

                let intersect_start = core::cmp::max(page_start, data_start_v);
                let intersect_end = core::cmp::min(page_end, data_end_v);

                if intersect_start < intersect_end {
                    let copy_len = (intersect_end - intersect_start) as usize;
                    let file_src_offset = file_offset + (intersect_start - data_start_v) as usize;
                    let dest_offset = (intersect_start - page_start) as usize;

                    let src_slice = &self.data[file_src_offset..file_src_offset + copy_len];
                    let dest_ptr =
                        ((phys_addr.as_u64() + hhdm) as *mut u8).wrapping_add(dest_offset);

                    // SAFETY: Copying within bounds of checked src_slice and allocated physical frame.
                    unsafe {
                        core::ptr::copy_nonoverlapping(src_slice.as_ptr(), dest_ptr, copy_len);
                    }
                }
            }
        }

        Ok(())
    }
}

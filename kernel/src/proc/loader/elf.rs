use super::header::*;
use crate::arch::paging::ArchPageTable;
use crate::mm::PageTable;
use crate::mm::{AddrSpace, MapFlags, VirtAddr, VmAreaKind};

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
        VirtAddr(self.header.e_entry)
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

    /// Retrieve the header of the section header string table.
    pub fn shstrtab_header(&self) -> Result<&'a Elf64Shdr, &'static str> {
        let sh_num = self.header.e_shnum as usize;
        let sh_str_ndx = self.header.e_shstrndx as usize;
        if sh_str_ndx >= sh_num {
            return Err("Invalid shstrndx in ELF header");
        }
        let sections = self.section_headers()?;
        Ok(&sections[sh_str_ndx])
    }

    /// Extract a null-terminated UTF-8 string from a string table section.
    pub fn get_string(
        &self,
        table_shdr: &Elf64Shdr,
        offset: usize,
    ) -> Result<&'a str, &'static str> {
        if table_shdr.sh_type != SHT_STRTAB {
            return Err("Section is not a string table");
        }
        let table_offset = table_shdr.sh_offset as usize;
        let table_size = table_shdr.sh_size as usize;
        if offset >= table_size {
            return Err("String offset out of bounds");
        }
        if table_offset + table_size > self.data.len() {
            return Err("String table data out of bounds");
        }

        let start = table_offset + offset;
        let mut end = start;
        while end < table_offset + table_size && self.data[end] != 0 {
            end += 1;
        }

        let slice = &self.data[start..end];
        core::str::from_utf8(slice).map_err(|_| "Invalid UTF-8 string in table")
    }

    /// Get the name of a section header.
    pub fn section_name(&self, shdr: &Elf64Shdr) -> Result<&'a str, &'static str> {
        let shstrtab = self.shstrtab_header()?;
        self.get_string(shstrtab, shdr.sh_name as usize)
    }

    /// Search for a section header by name.
    pub fn find_section(&self, name: &str) -> Result<Option<&'a Elf64Shdr>, &'static str> {
        let sections = self.section_headers()?;
        for shdr in sections {
            if self.section_name(shdr)? == name {
                return Ok(Some(shdr));
            }
        }
        Ok(None)
    }

    /// Maps the loadable segments, creates the user address space, allocates a user stack,
    /// sets up System V AMD64 ABI argc/argv/envp parameters, and returns loaded image information.
    pub fn load_with_cmdline(
        &self,
        cmdline: Option<&crate::proc::process::CommandLine>,
    ) -> Result<LoadedElf, &'static str> {
        let page_table = ArchPageTable::new().map_err(|_| "Failed to create PML4 page table")?;
        let mut addr_space = AddrSpace::new(page_table);

        self.load_segments(&mut addr_space)?;

        let stack_size = 256 * 1024; // 256 KiB stack
        let stack_top = VirtAddr(0x7FFF_FFFF_0000);
        let stack_start = stack_top - stack_size;
        let stack_flags = MapFlags::USER | MapFlags::READ | MapFlags::WRITE;

        addr_space
            .map_area(stack_start, stack_size, stack_flags, VmAreaKind::Anonymous)
            .map_err(|_| "Failed to map user stack VMA")?;

        let initial_sp = if let Some(cmd) = cmdline {
            Self::setup_user_stack(&mut addr_space, stack_top, cmd)?
        } else {
            stack_top
        };

        Ok(LoadedElf {
            entry_point: self.entry_point(),
            stack_pointer: initial_sp,
            addr_space,
        })
    }

    /// Maps the loadable segments, creates the user address space, allocates a user stack,
    /// and returns the loaded image information.
    pub fn load(&self) -> Result<LoadedElf, &'static str> {
        self.load_with_cmdline(None)
    }

    /// Setup the System V AMD64 ABI user stack frame with argc, argv, envp, and string tables.
    fn setup_user_stack(
        addr_space: &mut AddrSpace<ArchPageTable>,
        stack_top: VirtAddr,
        cmdline: &crate::proc::process::CommandLine,
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
                    .translate(VirtAddr(page_v))
                    .ok_or("Failed to translate user stack page")?;

                unsafe {
                    let dest = phys.as_ptr::<u8>(hhdm).add(page_off);
                    core::ptr::copy_nonoverlapping(bytes[written..].as_ptr(), dest, chunk_len);
                }
                written += chunk_len;
            }
            Ok(target_vaddr)
        };

        // 1. Push environment strings (null-terminated)
        let mut env_ptrs = alloc::vec::Vec::with_capacity(cmdline.env.len());
        for env_str in &cmdline.env {
            let mut str_bytes = alloc::vec::Vec::with_capacity(env_str.len() + 1);
            str_bytes.extend_from_slice(env_str.as_bytes());
            str_bytes.push(0);
            let str_vaddr = write_user_bytes(&mut cur_sp, &str_bytes)?;
            env_ptrs.push(str_vaddr);
        }

        // 2. Push argument strings (null-terminated)
        let mut arg_ptrs = alloc::vec::Vec::with_capacity(cmdline.args.len());
        for arg_str in &cmdline.args {
            let mut str_bytes = alloc::vec::Vec::with_capacity(arg_str.len() + 1);
            str_bytes.extend_from_slice(arg_str.as_bytes());
            str_bytes.push(0);
            let str_vaddr = write_user_bytes(&mut cur_sp, &str_bytes)?;
            arg_ptrs.push(str_vaddr);
        }

        // 3. Align cur_sp to 8 bytes
        cur_sp &= !7;

        // Calculate total table entries:
        // argc (1) + argv pointers (N) + NULL (1) + envp pointers (M) + NULL (1) + auxv (2)
        let total_entries = 1 + arg_ptrs.len() + 1 + env_ptrs.len() + 1 + 2;
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

        // 4. Push aux vector AT_NULL (0, 0)
        write_u64(&mut cur_sp, 0)?; // AT_NULL a_val
        write_u64(&mut cur_sp, 0)?; // AT_NULL a_type

        // 5. Push envp array (NULL-terminated)
        write_u64(&mut cur_sp, 0)?;
        for &ptr in env_ptrs.iter().rev() {
            write_u64(&mut cur_sp, ptr)?;
        }

        // 6. Push argv array (NULL-terminated)
        write_u64(&mut cur_sp, 0)?;
        for &ptr in arg_ptrs.iter().rev() {
            write_u64(&mut cur_sp, ptr)?;
        }

        // 7. Push argc
        write_u64(&mut cur_sp, arg_ptrs.len() as u64)?;

        Ok(VirtAddr(cur_sp))
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

    /// Load and map all PT_LOAD segments.
    fn load_segments(&self, addr_space: &mut AddrSpace<ArchPageTable>) -> Result<(), &'static str> {
        let ph_slice = self.program_headers()?;
        for phdr in ph_slice {
            if phdr.p_type == PT_LOAD {
                self.load_segment(addr_space, phdr)?;
            }
        }
        Ok(())
    }

    /// Map a single program segment and copy file data to allocated physical frames.
    fn load_segment(
        &self,
        addr_space: &mut AddrSpace<ArchPageTable>,
        phdr: &Elf64Phdr,
    ) -> Result<(), &'static str> {
        let start_vaddr = VirtAddr(phdr.p_vaddr);
        let end_vaddr = start_vaddr + phdr.p_memsz as usize;

        let aligned_start = VirtAddr(phdr.p_vaddr & !4095);
        let aligned_end = VirtAddr((end_vaddr.as_u64() + 4095) & !4095);
        let aligned_size = (aligned_end - aligned_start) as usize;

        if aligned_size == 0 {
            return Ok(());
        }

        let mut flags = MapFlags::USER;
        if (phdr.p_flags & 4) != 0 {
            flags |= MapFlags::READ;
        }
        if (phdr.p_flags & 2) != 0 {
            flags |= MapFlags::WRITE;
        }
        if (phdr.p_flags & 1) != 0 {
            flags |= MapFlags::EXECUTE;
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
                let page_virt = VirtAddr(page_virt_u64);
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
                    let dest_ptr = phys_addr.as_ptr::<u8>(hhdm);

                    // SAFETY: Copying within bounds of checked src_slice and allocated physical frame.
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            src_slice.as_ptr(),
                            dest_ptr.add(dest_offset),
                            copy_len,
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

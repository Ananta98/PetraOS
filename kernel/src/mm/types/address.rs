use core::ops::{Add, AddAssign, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(pub u64);

impl PhysAddr {
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub fn as_ptr<T>(self, hhdm_offset: u64) -> *mut T {
        // SAFETY: Direct map addresses are mapped into virtual address space by the bootloader.
        (self.0 + hhdm_offset) as *mut T
    }

    pub fn from_ptr<T>(ptr: *const T, hhdm_offset: u64) -> Self {
        Self(ptr as u64 - hhdm_offset)
    }

    pub fn is_aligned(self, align: u64) -> bool {
        self.0 % align == 0
    }
}

impl VirtAddr {
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub fn as_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }

    pub fn is_aligned(self, align: u64) -> bool {
        self.0 % align == 0
    }
}

// Implementations for PhysAddr
impl Add<u64> for PhysAddr {
    type Output = Self;
    fn add(self, rhs: u64) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl Add<usize> for PhysAddr {
    type Output = Self;
    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs as u64)
    }
}

impl AddAssign<u64> for PhysAddr {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl AddAssign<usize> for PhysAddr {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs as u64;
    }
}

impl Sub<u64> for PhysAddr {
    type Output = Self;
    fn sub(self, rhs: u64) -> Self::Output {
        Self(self.0 - rhs)
    }
}

impl Sub<usize> for PhysAddr {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self::Output {
        Self(self.0 - rhs as u64)
    }
}

impl SubAssign<u64> for PhysAddr {
    fn sub_assign(&mut self, rhs: u64) {
        self.0 -= rhs;
    }
}

impl SubAssign<usize> for PhysAddr {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 -= rhs as u64;
    }
}

impl Sub<PhysAddr> for PhysAddr {
    type Output = u64;
    fn sub(self, rhs: PhysAddr) -> Self::Output {
        self.0 - rhs.0
    }
}

// Implementations for VirtAddr
impl Add<u64> for VirtAddr {
    type Output = Self;
    fn add(self, rhs: u64) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl Add<usize> for VirtAddr {
    type Output = Self;
    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs as u64)
    }
}

impl AddAssign<u64> for VirtAddr {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl AddAssign<usize> for VirtAddr {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs as u64;
    }
}

impl Sub<u64> for VirtAddr {
    type Output = Self;
    fn sub(self, rhs: u64) -> Self::Output {
        Self(self.0 - rhs)
    }
}

impl Sub<usize> for VirtAddr {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self::Output {
        Self(self.0 - rhs as u64)
    }
}

impl SubAssign<u64> for VirtAddr {
    fn sub_assign(&mut self, rhs: u64) {
        self.0 -= rhs;
    }
}

impl SubAssign<usize> for VirtAddr {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 -= rhs as u64;
    }
}

impl Sub<VirtAddr> for VirtAddr {
    type Output = u64;
    fn sub(self, rhs: VirtAddr) -> Self::Output {
        self.0 - rhs.0
    }
}

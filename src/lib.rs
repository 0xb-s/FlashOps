#![no_std]
#![no_main]
#![macro_use]

#[cfg(all(not(test), feature = "panic-handler"))]
#[panic_handler]
fn handle_panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        core::arch::asm!("udf #0");
        core::hint::unreachable_unchecked();
    }
}

pub const ERASE: u32 = 1;
pub const PROGRAM: u32 = 2;
pub const VERIFY: u32 = 3;

pub type Error = core::num::NonZeroU32;

pub trait FlashOps {
    fn create(address: u32, clock: u32, operation: Operation) -> Result<Self, Error>
    where
        Self: Sized;

    #[cfg(feature = "erase-chip")]
    fn erase_chip(&mut self) -> Result<(), Error>;

    fn erase_sector(&mut self, address: u32) -> Result<(), Error>;
    fn program_page(&mut self, address: u32, data: &[u8]) -> Result<(), Error>;

    #[cfg(feature = "verify")]
    fn verify(&mut self, address: u32, size: u32, data: Option<&[u8]>) -> Result<(), Error>;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Operation {
    Erase = 1,
    Program = 2,
    Verify = 3,
}

#[macro_export]
macro_rules! flash_algorithm {
    ($algo:ty, {flash_address: $addr:expr, flash_size: $size:expr, page_size: $page_size:expr, empty_value: $empty:expr, sectors: [$({size: $sector_size:expr, address: $sector_addr:expr}),+]}) => {
        static mut INIT_FLAG: bool = false;
        static mut ALGO_INSTANCE: core::mem::MaybeUninit<$algo> = core::mem::MaybeUninit::uninit();

        #[no_mangle]
        #[link_section = ".entry"]
        pub unsafe extern "C" fn initialize(addr: u32, clock: u32, op: u32) -> u32 {
            if INIT_FLAG {
                deinitialize();
            }
            INIT_FLAG = true;
            let op = match op {
                1 => $crate::Operation::Erase,
                2 => $crate::Operation::Program,
                3 => $crate::Operation::Verify,
                _ => panic!("Invalid operation code.")
            };
            match <$algo as FlashOps>::create(addr, clock, op) {
                Ok(instance) => {
                    ALGO_INSTANCE.as_mut_ptr().write(instance);
                    INIT_FLAG = true;
                    0
                }
                Err(e) => e.get(),
            }
        }

        #[no_mangle]
        #[link_section = ".entry"]
        pub unsafe extern "C" fn deinitialize() -> u32 {
            if !INIT_FLAG {
                return 1;
            }
            ALGO_INSTANCE.as_mut_ptr().drop_in_place();
            INIT_FLAG = false;
            0
        }

        #[no_mangle]
        #[link_section = ".entry"]
        pub unsafe extern "C" fn erase_sector(addr: u32) -> u32 {
            if !INIT_FLAG {
                return 1;
            }
            let instance = &mut *ALGO_INSTANCE.as_mut_ptr();
            match <$algo as FlashOps>::erase_sector(instance, addr) {
                Ok(()) => 0,
                Err(e) => e.get(),
            }
        }

        #[no_mangle]
        #[link_section = ".entry"]
        pub unsafe extern "C" fn program_page(addr: u32, size: u32, data: *const u8) -> u32 {
            if !INIT_FLAG {
                return 1;
            }
            let instance = &mut *ALGO_INSTANCE.as_mut_ptr();
            let data_slice: &[u8] = core::slice::from_raw_parts(data, size as usize);
            match <$algo as FlashOps>::program_page(instance, addr, data_slice) {
                Ok(()) => 0,
                Err(e) => e.get(),
            }
        }

        $crate::erase_chip!($algo);
        $crate::verify!($algo);

        #[allow(non_upper_case_globals)]
        #[no_mangle]
        #[used]
        #[link_section = "DeviceData"]
        pub static FlashDeviceInfo: FlashDevice = FlashDevice {
            vers: 0x0,
            dev_name: [0u8; 128],
            dev_type: 5,
            dev_addr: $addr,
            device_size: $size,
            page_size: $page_size,
            _reserved: 0,
            empty: $empty,
            program_time_out: 1000,
            erase_time_out: 2000,
            flash_sectors: [
                $(
                    Sector { size: $sector_size, address: $sector_addr }
                ),+,
                Sector {
                    size: 0xffff_ffff,
                    address: 0xffff_ffff,
                }
            ],
        };

        #[repr(C)]
        pub struct FlashDevice {
            vers: u16,
            dev_name: [u8; 128],
            dev_type: u16,
            dev_addr: u32,
            device_size: u32,
            page_size: u32,
            _reserved: u32,
            empty: u8,
            program_time_out: u32,
            erase_time_out: u32,
            flash_sectors: [Sector; $crate::count!($($sector_size)*) + 1],
        }

        #[repr(C)]
        #[derive(Copy, Clone)]
        pub struct Sector {
            size: u32,
            address: u32,
        }
    };
}

#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "erase-chip"))]
macro_rules! erase_chip {
    ($type:ty) => {};
}

#[doc(hidden)]
#[macro_export]
#[cfg(feature = "erase-chip")]
macro_rules! erase_chip {
    ($type:ty) => {
        #[no_mangle]
        #[link_section = ".entry"]
        pub unsafe extern "C" fn erase_chip() -> u32 {
            if !INIT_FLAG {
                return 1;
            }
            let instance = &mut *ALGO_INSTANCE.as_mut_ptr();
            match <$type as FlashOps>::erase_chip(instance) {
                Ok(()) => 0,
                Err(e) => e.get(),
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "verify"))]
macro_rules! verify {
    ($type:ty) => {};
}

#[doc(hidden)]
#[macro_export]
#[cfg(feature = "verify")]
macro_rules! verify {
    ($type:ty) => {
        #[no_mangle]
        #[link_section = ".entry"]
        pub unsafe extern "C" fn verify(addr: u32, size: u32, data: *const u8) -> u32 {
            if !INIT_FLAG {
                return 1;
            }
            let instance = &mut *ALGO_INSTANCE.as_mut_ptr();
            let data_slice = if data.is_null() {
                None
            } else {
                Some(unsafe { core::slice::from_raw_parts(data, size as usize) })
            };
            match <$type as FlashOps>::verify(instance, addr, size, data_slice) {
                Ok(()) => 0,
                Err(e) => e.get(),
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! count {
    () => (0usize);
    ( $x:tt $($xs:tt)* ) => (1usize + count!($($xs)*));
}

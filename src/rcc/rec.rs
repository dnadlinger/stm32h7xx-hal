//! Peripheral Reset and Enable Control (REC)
//!
//! This module contains safe accessors to the RCC functionality for each
//! peripheral.
//!
//! At a minimum each peripheral implements
//! [ResetEnable](trait.ResetEnable.html). Peripherals that have an
//! individual clock multiplexer in the PKSU also have methods
//! `kernel_clk_mux` and `get_kernel_clk_mux`. These set and get the state
//! of the kernel clock multiplexer respectively.
//!
//! Peripherals that share a clock multiplexer in the PKSU with other
//! peripherals implement a trait with a `get_kernel_clk_mux` method that
//! returns the current kernel clock state. Because the kernel_clk_mux is shared
//! between multiple peripherals, it cannot be set by any individual one of
//! them. Instead it can only be set by methods on the
//! [`PeripheralRec`](struct.PeripheralRec.html) itself. These methods are named
//! `kernel_xxxx_clk_mux()`.
//!
//! # Reset/Enable Example
//!
//! ```
//! // Constrain and Freeze power
//! ...
//! let rcc = dp.RCC.constrain();
//! let ccdr = rcc.sys_ck(100.mhz()).freeze(vos, &dp.SYSCFG);
//!
//! // Enable the clock to a peripheral and reset it
//! ccdr.peripheral.FDCAN.enable().reset();
//! ```
//!
//! # Individual Kernel Clock Example
//! ```
//! let ccdr = ...; // Returned by `freeze()`, see example above
//!
//! // Set individual kernel clock
//! let cec_prec = ccdr.peripheral.CEC.kernel_clk_mux(CecClkSel::LSI);
//!
//! assert_eq!(cec_prec.get_kernel_clk_mux(), CecClkSel::LSI);
//! ```
//!
//! # Group Kernel Clock Example
//! ```
//! let mut ccdr = ...; // Returned by `freeze()`, see example above
//!
//! // Set group kernel clock mux
//! ccdr.peripheral.kernel_i2c123_clk_mux(I2c123ClkSel::PLL3_R);
//!
//! // Enable and reset peripheral
//! let i2c3_prec = ccdr.peripheral.enable().reset();
//! assert_eq!(i2c3_prec.get_kernel_clk_mux(), I2c123ClkSel::PLL3_R);
//!
//! init_i2c3(..., i2c3_prec);
//!
//! // Can't set group kernel clock (it would also affect I2C3)
//! // ccdr.peripheral.kernel_i2c123_clk_mux(I2c123ClkSel::HSI_KER);
//! ```
#![deny(missing_docs)]

use core::marker::PhantomData;

use super::Rcc;
use crate::stm32::{rcc, RCC};
use cortex_m::interrupt;

/// A trait for Resetting, Enabling and Disabling a single peripheral
pub trait ResetEnable {
    /// Enable this peripheral
    fn enable(self) -> Self;
    /// Disable this peripheral
    fn disable(self) -> Self;
    /// Reset this peripheral
    fn reset(self) -> Self;
}

/// The clock gating state of a peripheral in low-power mode
///
/// See RM0433 rev 7. Section 8.5.11
#[derive(Copy, Clone, PartialEq)]
pub enum LowPowerMode {
    /// Kernel and bus interface clocks are not provided in low-power modes.
    Off,
    /// Kernel and bus interface clocks are provided in CSleep mode.
    Enabled,
    /// Kernel and bus interface clocks are provided in both CSleep and CStop
    /// modes. Only applies to peripherals in the D3 / SRD. If the peripheral is
    /// not in the D3 / SRD then this has the same effect as `Enabled`.
    Autonomous,
}
impl Default for LowPowerMode {
    fn default() -> Self {
        LowPowerMode::Enabled
    }
}

impl Rcc {
    /// Returns all the peripherals resets / enables / kernel clocks.
    ///
    /// # Use case
    ///
    /// Allows peripherals to be reset / enabled before the calling
    /// freeze. For example, the internal watchdog could be enabled to
    /// issue a reset if the call the freeze hangs waiting for an external
    /// clock that is stopped.
    ///
    /// # Safety
    ///
    /// If this method is called multiple times, or is called before the
    /// [freeze](struct.Rcc.html#freeze), then multiple accesses to the
    /// same memory exist.
    #[inline]
    pub unsafe fn steal_peripheral_rec(&self) -> PeripheralREC {
        PeripheralREC::new_singleton()
    }
}

// This macro uses the paste::item! macro to create identifiers.
//
// https://crates.io/crates/paste
macro_rules! peripheral_reset_and_enable_control {
    ($($AXBn:ident, $axb_doc:expr => [
        $(
            $( #[ $pmeta:meta ] )*
                $(($Auto:ident))* $p:ident
                $([ kernel $clk:ident: $pk:ident $(($Variant:ident))* $ccip:ident $clk_doc:expr ])*
                $([ group clk: $pk_g:ident $( $(($Variant_g:ident))* $ccip_g:ident $clk_doc_g:expr )* ])*
        ),*
    ];)+) => {
        paste::item! {
            /// Peripheral Reset and Enable Control
            #[allow(non_snake_case)]
            #[non_exhaustive]
            pub struct PeripheralREC {
                $(
                    $(
                        #[allow(missing_docs)]
                        $( #[ $pmeta ] )*
                        pub [< $p:upper >]: $p,
                    )*
                )+
            }
            impl PeripheralREC {
                /// Return a new instance of the peripheral resets /
                /// enables / kernel clocks
                ///
                /// # Safety
                ///
                /// If this method is called multiple times, then multiple
                /// accesses to the same memory exist.
                pub(super) unsafe fn new_singleton() -> PeripheralREC {
                    PeripheralREC {
                        $(
                            $(
                                $( #[ $pmeta ] )*
                                [< $p:upper >]: $p {
                                    _marker: PhantomData,
                                },
                            )*
                        )+
                    }
                }
            }
            $(
                $(
                    /// Owned ability to Reset, Enable and Disable peripheral
                    $( #[ $pmeta ] )*
                    pub struct $p {
                        pub(crate) _marker: PhantomData<*const ()>,
                    }
                    $( #[ $pmeta ] )*
                    impl $p {
                        /// Set Low Power Mode for peripheral
                        pub fn low_power(self, lpm: LowPowerMode) -> Self {
                            // unsafe: Owned exclusive access to this bitfield
                            interrupt::free(|_| {
                                // LPEN
                                let lpenr = unsafe {
                                    &(*RCC::ptr()).[< $AXBn:lower lpenr >]
                                };
                                lpenr.modify(|_, w| w.[< $p:lower lpen >]()
                                             .bit(lpm != LowPowerMode::Off));
                                // AMEN
                                $(
                                    let amr = unsafe { autonomous!($Auto) };
                                    amr.modify(|_, w| w.[< $p:lower amen >]()
                                               .bit(lpm == LowPowerMode::Autonomous));
                                )*
                            });
                            self
                        }
                    }
                    $( #[ $pmeta ] )*
                    unsafe impl Send for $p {}
                    $( #[ $pmeta ] )*
                    impl ResetEnable for $p {
                        #[inline(always)]
                        fn enable(self) -> Self {
                            // unsafe: Owned exclusive access to this bitfield
                            interrupt::free(|_| {
                                let enr = unsafe {
                                    &(*RCC::ptr()).[< $AXBn:lower enr >]
                                };
                                enr.modify(|_, w| w.
                                           [< $p:lower en >]().set_bit());
                            });
                            self
                        }
                        #[inline(always)]
                        fn disable(self) -> Self {
                            // unsafe: Owned exclusive access to this bitfield
                            interrupt::free(|_| {
                                let enr = unsafe {
                                    &(*RCC::ptr()).[< $AXBn:lower enr >]
                                };
                                enr.modify(|_, w| w.
                                           [< $p:lower en >]().clear_bit());
                            });
                            self
                        }
                        #[inline(always)]
                        fn reset(self) -> Self {
                            // unsafe: Owned exclusive access to this bitfield
                            interrupt::free(|_| {
                                let rstr = unsafe {
                                    &(*RCC::ptr()).[< $AXBn:lower rstr >]
                                };
                                rstr.modify(|_, w| w.
                                            [< $p:lower rst >]().set_bit());
                                rstr.modify(|_, w| w.
                                            [< $p:lower rst >]().clear_bit());
                            });
                            self
                        }
                    }
                    $( #[ $pmeta ] )*
                    impl $p {
                        $(      // Individual kernel clocks
                            #[inline(always)]
                            /// Modify a kernel clock for this
                            /// peripheral. See RM0433 Section 8.5.8.
                            ///
                            /// It is possible to switch this clock
                            /// dynamically without generating spurs or
                            /// timing violations. However, the user must
                            /// ensure that both clocks are running. See
                            /// RM0433 Section 8.5.10
                            pub fn [< kernel_ $clk _mux >](self, sel: [< $pk ClkSel >]) -> Self {
                                // unsafe: Owned exclusive access to this bitfield
                                interrupt::free(|_| {
                                    let ccip = unsafe {
                                        &(*RCC::ptr()).[< $ccip r >]
                                    };
                                    ccip.modify(|_, w| w.
                                                [< $pk:lower sel >]().variant(sel));
                                });
                                self
                            }

                            #[inline(always)]
                            /// Return the current kernel clock selection
                            pub fn [< get_kernel_ $clk _mux>](&self) ->
                                variant_return_type!([< $pk ClkSel >] $(, $Variant)*)
                            {
                                // unsafe: We only read from this bitfield
                                let ccip = unsafe {
                                    &(*RCC::ptr()).[< $ccip r >]
                                };
                                ccip.read().[< $pk:lower sel >]().variant()
                            }
                        )*
                    }
                    $(          // Individual kernel clocks
                        #[doc=$clk_doc]
                        /// kernel clock source selection
                        pub type [< $pk ClkSel >] =
                            rcc::[< $ccip r >]::[< $pk:upper SEL_A >];
                    )*

                    $(          // Group kernel clocks
                        impl [< $pk_g ClkSelGetter >] for $p {}
                    )*
                    $(          // Group kernel clocks
                        $(
                            #[doc=$clk_doc_g]
                            /// kernel clock source selection
                            pub type [< $pk_g ClkSel >] =
                                rcc::[< $ccip_g r >]::[< $pk_g:upper SEL_A >];

                            /// Can return
                            #[doc=$clk_doc_g]
                            /// kernel clock source selection
                            pub trait [< $pk_g ClkSelGetter >] {
                                #[inline(always)]
                                #[allow(unused)]
                                /// Return the
                                #[doc=$clk_doc_g]
                                /// kernel clock selection
                                fn get_kernel_clk_mux(&self) ->
                                    variant_return_type!([< $pk_g ClkSel >] $(, $Variant_g)*)
                                {
                                    // unsafe: We only read from this bitfield
                                    let ccip = unsafe {
                                        &(*RCC::ptr()).[< $ccip_g r >]
                                    };
                                    ccip.read().[< $pk_g:lower sel >]().variant()
                                }
                            }
                        )*
                    )*
                    impl PeripheralREC {
                        $(          // Group kernel clocks
                            $(
                                /// Modify the kernel clock for
                                #[doc=$clk_doc_g]
                                /// . See RM0433 Section 8.5.8.
                                ///
                                /// It is possible to switch this clock
                                /// dynamically without generating spurs or
                                /// timing violations. However, the user must
                                /// ensure that both clocks are running. See
                                /// RM0433 Section 8.5.10
                                pub fn [< kernel_ $pk_g:lower _clk_mux >](&mut self, sel: [< $pk_g ClkSel >]) -> &mut Self {
                                    // unsafe: Owned exclusive access to this bitfield
                                    interrupt::free(|_| {
                                        let ccip = unsafe {
                                            &(*RCC::ptr()).[< $ccip_g r >]
                                        };
                                        ccip.modify(|_, w| w.
                                                    [< $pk_g:lower sel >]().variant(sel));
                                    });
                                    self
                                }
                            )*
                        )*
                    }
                )*
            )+
        }
    }
}

// If the PAC does not fully specify a CCIP field (perhaps because one or
// more values are reserved), then we use a different return type
macro_rules! variant_return_type {
    ($t:ty) => { $t };
    ($t:ty, $Variant: ident) => {
        stm32h7::Variant<u8, $t>
    };
}

// Register for autonomous mode enable bits
macro_rules! autonomous {
    ($Auto:ident) => {
        &(*RCC::ptr()).d3amr
    };
}

// Enumerate all peripherals and optional clock multiplexers
//
// If a kernel clock multiplexer is shared between multiple peripherals, all
// those peripherals must be marked with a common group clk.
peripheral_reset_and_enable_control! {
    AHB1, "AMBA High-performance Bus (AHB1) peripherals" => [
        Eth1Mac, Dma2, Dma1,
        #[cfg(any(feature = "dualcore"))] Art,
        Adc12 [group clk: Adc(Variant) d3ccip "ADC"]
    ];

    AHB2, "AMBA High-performance Bus (AHB2) peripherals" => [
        Hash, Crypt,
        Rng [kernel clk: Rng d2ccip2 "RNG"],
        Sdmmc2 [group clk: Sdmmc]
    ];

    AHB3, "AMBA High-performance Bus (AHB3) peripherals" => [
        Sdmmc1 [group clk: Sdmmc d1ccip "SDMMC"],
        Qspi [kernel clk: Qspi d1ccip "QUADSPI"],
        Fmc [kernel clk: Fmc d1ccip "FMC"],
        Jpgdec, Dma2d, Mdma
    ];

    AHB4, "AMBA High-performance Bus (AHB4) peripherals" => [
        (Auto) Bdma,
        (Auto) Crc,
        (Auto) Adc3 [group clk: Adc],

        Gpioa, Gpiob, Gpioc, Gpiod, Gpioe, Gpiof, Gpiog, Gpioh, Gpioi, Gpioj, Gpiok
    ];

    APB1L, "Advanced Peripheral Bus 1L (APB1L) peripherals" => [
        Dac12,
        I2c1 [group clk: I2c123 d2ccip2 "I2C1/2/3"],
        I2c2 [group clk: I2c123],
        I2c3 [group clk: I2c123],

        Cec [kernel clk: Cec(Variant) d2ccip2 "CEC"],
        Lptim1 [kernel clk: Lptim1(Variant) d2ccip2 "LPTIM1"],

        Spi2 [group clk: Spi123],
        Spi3 [group clk: Spi123],

        Tim2, Tim3, Tim4, Tim5, Tim6, Tim7, Tim12, Tim13, Tim14,

        Usart2 [group clk: Usart234578(Variant) d2ccip2 "USART2/3/4/5/7/8"],
        Usart3 [group clk: Usart234578],
        Uart4 [group clk: Usart234578],
        Uart5 [group clk: Usart234578],
        Uart7 [group clk: Usart234578],
        Uart8 [group clk: Usart234578]
    ];

    APB1H, "Advanced Peripheral Bus 1H (APB1H) peripherals" => [
        Fdcan [kernel clk: Fdcan(Variant) d2ccip1 "FDCAN"],
        Swp [kernel clk: Swp d2ccip1 "SWPMI"],
        Crs, Mdios, Opamp
    ];

    APB2, "Advanced Peripheral Bus 2 (APB2) peripherals" => [
        Hrtim,
        Dfsdm1 [kernel clk: Dfsdm1 d2ccip1 "DFSDM1"],

        Sai1 [kernel clk: Sai1(Variant) d2ccip1 "SAI1"],
        Sai2 [group clk: Sai23(Variant) d2ccip1 "SAI2/3"],
        Sai3 [group clk: Sai23],

        Spi1 [group clk: Spi123(Variant) d2ccip1 "SPI1/2/3"],
        Spi4 [group clk: Spi45(Variant) d2ccip1 "SPI4/5"],
        Spi5 [group clk: Spi45],

        Tim1, Tim8, Tim15, Tim16, Tim17,

        Usart1 [group clk: Usart16(Variant) d2ccip2 "USART1/6"],
        Usart6 [group clk: Usart16]
    ];

    APB3, "Advanced Peripheral Bus 3 (APB3) peripherals" => [
        Ltdc,
        #[cfg(any(feature = "dsi"))] Dsi
    ];

    APB4, "Advanced Peripheral Bus 4 (APB4) peripherals" => [
        (Auto) Vref,
        (Auto) Comp12,

        (Auto) Lptim2 [kernel clk: Lptim2(Variant) d3ccip "LPTIM2"],
        (Auto) Lptim3 [group clk: Lptim345(Variant) d3ccip "LPTIM3/4/5"],
        (Auto) Lptim4 [group clk: Lptim345],
        (Auto) Lptim5 [group clk: Lptim345],
        (Auto) I2c4 [kernel clk: I2c4 d3ccip "I2C4"],
        (Auto) Spi6 [kernel clk: Spi6(Variant) d3ccip "SPI6"],
        (Auto) Sai4 [kernel clk_a: Sai4A(Variant) d3ccip
            "Sub-Block A of SAI4"]
            [kernel clk_b: Sai4B(Variant) d3ccip
            "Sub-Block B of SAI4"]
    ];
}

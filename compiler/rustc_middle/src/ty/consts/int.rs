use rustc_apfloat::ieee::{Double, Half, Quad, Single};
use rustc_apfloat::Float;
use rustc_errors::{DiagArgValue, IntoDiagArg};
use rustc_serialize::{Decodable, Decoder, Encodable, Encoder};
use rustc_target::abi::Size;
use std::fmt;
use std::num::NonZero;

use crate::ty::TyCtxt;

#[derive(Copy, Clone)]
/// A type for representing any integer. Only used for printing.
pub struct ConstInt {
    /// The "untyped" variant of `ConstInt`.
    int: ScalarInt,
    /// Whether the value is of a signed integer type.
    signed: bool,
    /// Whether the value is a `usize` or `isize` type.
    is_ptr_sized_integral: bool,
}

impl ConstInt {
    pub fn new(int: ScalarInt, signed: bool, is_ptr_sized_integral: bool) -> Self {
        Self { int, signed, is_ptr_sized_integral }
    }
}

impl std::fmt::Debug for ConstInt {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { int, signed, is_ptr_sized_integral } = *self;
        let size = int.size().bytes();
        let raw = int.data;
        if signed {
            let bit_size = size * 8;
            let min = 1u128 << (bit_size - 1);
            let max = min - 1;
            if raw == min {
                match (size, is_ptr_sized_integral) {
                    (_, true) => write!(fmt, "isize::MIN"),
                    (1, _) => write!(fmt, "i8::MIN"),
                    (2, _) => write!(fmt, "i16::MIN"),
                    (4, _) => write!(fmt, "i32::MIN"),
                    (8, _) => write!(fmt, "i64::MIN"),
                    (16, _) => write!(fmt, "i128::MIN"),
                    _ => bug!("ConstInt 0x{:x} with size = {} and signed = {}", raw, size, signed),
                }
            } else if raw == max {
                match (size, is_ptr_sized_integral) {
                    (_, true) => write!(fmt, "isize::MAX"),
                    (1, _) => write!(fmt, "i8::MAX"),
                    (2, _) => write!(fmt, "i16::MAX"),
                    (4, _) => write!(fmt, "i32::MAX"),
                    (8, _) => write!(fmt, "i64::MAX"),
                    (16, _) => write!(fmt, "i128::MAX"),
                    _ => bug!("ConstInt 0x{:x} with size = {} and signed = {}", raw, size, signed),
                }
            } else {
                match size {
                    1 => write!(fmt, "{}", raw as i8)?,
                    2 => write!(fmt, "{}", raw as i16)?,
                    4 => write!(fmt, "{}", raw as i32)?,
                    8 => write!(fmt, "{}", raw as i64)?,
                    16 => write!(fmt, "{}", raw as i128)?,
                    _ => bug!("ConstInt 0x{:x} with size = {} and signed = {}", raw, size, signed),
                }
                if fmt.alternate() {
                    match (size, is_ptr_sized_integral) {
                        (_, true) => write!(fmt, "_isize")?,
                        (1, _) => write!(fmt, "_i8")?,
                        (2, _) => write!(fmt, "_i16")?,
                        (4, _) => write!(fmt, "_i32")?,
                        (8, _) => write!(fmt, "_i64")?,
                        (16, _) => write!(fmt, "_i128")?,
                        (sz, _) => bug!("unexpected int size i{sz}"),
                    }
                }
                Ok(())
            }
        } else {
            let max = Size::from_bytes(size).truncate(u128::MAX);
            if raw == max {
                match (size, is_ptr_sized_integral) {
                    (_, true) => write!(fmt, "usize::MAX"),
                    (1, _) => write!(fmt, "u8::MAX"),
                    (2, _) => write!(fmt, "u16::MAX"),
                    (4, _) => write!(fmt, "u32::MAX"),
                    (8, _) => write!(fmt, "u64::MAX"),
                    (16, _) => write!(fmt, "u128::MAX"),
                    _ => bug!("ConstInt 0x{:x} with size = {} and signed = {}", raw, size, signed),
                }
            } else {
                match size {
                    1 => write!(fmt, "{}", raw as u8)?,
                    2 => write!(fmt, "{}", raw as u16)?,
                    4 => write!(fmt, "{}", raw as u32)?,
                    8 => write!(fmt, "{}", raw as u64)?,
                    16 => write!(fmt, "{}", raw as u128)?,
                    _ => bug!("ConstInt 0x{:x} with size = {} and signed = {}", raw, size, signed),
                }
                if fmt.alternate() {
                    match (size, is_ptr_sized_integral) {
                        (_, true) => write!(fmt, "_usize")?,
                        (1, _) => write!(fmt, "_u8")?,
                        (2, _) => write!(fmt, "_u16")?,
                        (4, _) => write!(fmt, "_u32")?,
                        (8, _) => write!(fmt, "_u64")?,
                        (16, _) => write!(fmt, "_u128")?,
                        (sz, _) => bug!("unexpected unsigned int size u{sz}"),
                    }
                }
                Ok(())
            }
        }
    }
}

impl IntoDiagArg for ConstInt {
    // FIXME this simply uses the Debug impl, but we could probably do better by converting both
    // to an inherent method that returns `Cow`.
    fn into_diag_arg(self) -> DiagArgValue {
        DiagArgValue::Str(format!("{self:?}").into())
    }
}

/// The raw bytes of a simple value.
///
/// This is a packed struct in order to allow this type to be optimally embedded in enums
/// (like Scalar).
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
#[repr(packed)]
pub struct ScalarInt {
    /// The first `size` bytes of `data` are the value.
    /// Do not try to read less or more bytes than that. The remaining bytes must be 0.
    data: u128,
    size: NonZero<u8>,
}

// Cannot derive these, as the derives take references to the fields, and we
// can't take references to fields of packed structs.
impl<CTX> crate::ty::HashStable<CTX> for ScalarInt {
    fn hash_stable(&self, hcx: &mut CTX, hasher: &mut crate::ty::StableHasher) {
        // Using a block `{self.data}` here to force a copy instead of using `self.data`
        // directly, because `hash_stable` takes `&self` and would thus borrow `self.data`.
        // Since `Self` is a packed struct, that would create a possibly unaligned reference,
        // which is UB.
        { self.data }.hash_stable(hcx, hasher);
        self.size.get().hash_stable(hcx, hasher);
    }
}

impl<S: Encoder> Encodable<S> for ScalarInt {
    fn encode(&self, s: &mut S) {
        let size = self.size.get();
        s.emit_u8(size);
        s.emit_raw_bytes(&self.data.to_le_bytes()[..size as usize]);
    }
}

impl<D: Decoder> Decodable<D> for ScalarInt {
    fn decode(d: &mut D) -> ScalarInt {
        let mut data = [0u8; 16];
        let size = d.read_u8();
        data[..size as usize].copy_from_slice(d.read_raw_bytes(size as usize));
        ScalarInt { data: u128::from_le_bytes(data), size: NonZero::new(size).unwrap() }
    }
}

impl ScalarInt {
    pub const TRUE: ScalarInt = ScalarInt { data: 1_u128, size: NonZero::new(1).unwrap() };
    pub const FALSE: ScalarInt = ScalarInt { data: 0_u128, size: NonZero::new(1).unwrap() };

    fn raw(data: u128, size: Size) -> Self {
        Self { data, size: NonZero::new(size.bytes() as u8).unwrap() }
    }

    #[inline]
    pub fn size(self) -> Size {
        Size::from_bytes(self.size.get())
    }

    /// Make sure the `data` fits in `size`.
    /// This is guaranteed by all constructors here, but having had this check saved us from
    /// bugs many times in the past, so keeping it around is definitely worth it.
    #[inline(always)]
    fn check_data(self) {
        // Using a block `{self.data}` here to force a copy instead of using `self.data`
        // directly, because `debug_assert_eq` takes references to its arguments and formatting
        // arguments and would thus borrow `self.data`. Since `Self`
        // is a packed struct, that would create a possibly unaligned reference, which
        // is UB.
        debug_assert_eq!(
            self.size().truncate(self.data),
            { self.data },
            "Scalar value {:#x} exceeds size of {} bytes",
            { self.data },
            self.size
        );
    }

    #[inline]
    pub fn null(size: Size) -> Self {
        Self::raw(0, size)
    }

    #[inline]
    pub fn is_null(self) -> bool {
        self.data == 0
    }

    #[inline]
    pub fn try_from_uint(i: impl Into<u128>, size: Size) -> Option<Self> {
        let data = i.into();
        if size.truncate(data) == data { Some(Self::raw(data, size)) } else { None }
    }

    /// Returns the truncated result, and whether truncation changed the value.
    #[inline]
    pub fn truncate_from_uint(i: impl Into<u128>, size: Size) -> (Self, bool) {
        let data = i.into();
        let r = Self::raw(size.truncate(data), size);
        (r, r.data != data)
    }

    #[inline]
    pub fn try_from_int(i: impl Into<i128>, size: Size) -> Option<Self> {
        let i = i.into();
        // `into` performed sign extension, we have to truncate
        let truncated = size.truncate(i as u128);
        if size.sign_extend(truncated) as i128 == i {
            Some(Self::raw(truncated, size))
        } else {
            None
        }
    }

    /// Returns the truncated result, and whether truncation changed the value.
    #[inline]
    pub fn truncate_from_int(i: impl Into<i128>, size: Size) -> (Self, bool) {
        let data = i.into();
        let r = Self::raw(size.truncate(data as u128), size);
        (r, size.sign_extend(r.data) as i128 != data)
    }

    #[inline]
    pub fn try_from_target_usize(i: impl Into<u128>, tcx: TyCtxt<'_>) -> Option<Self> {
        Self::try_from_uint(i, tcx.data_layout.pointer_size)
    }

    #[inline]
    pub fn try_to_bits(self, target_size: Size) -> Result<u128, Size> {
        assert_ne!(target_size.bytes(), 0, "you should never look at the bits of a ZST");
        if target_size.bytes() == u64::from(self.size.get()) {
            self.check_data();
            Ok(self.data)
        } else {
            Err(self.size())
        }
    }

    #[inline]
    pub fn assert_bits(self, target_size: Size) -> u128 {
        self.try_to_bits(target_size).unwrap_or_else(|size| {
            bug!("expected int of size {}, but got size {}", target_size.bytes(), size.bytes())
        })
    }

    /// Tries to convert the `ScalarInt` to an unsigned integer of the given size.
    /// Fails if the size of the `ScalarInt` is not equal to `size` and returns the
    /// `ScalarInt`s size in that case.
    #[inline]
    pub fn try_to_uint(self, size: Size) -> Result<u128, Size> {
        self.try_to_bits(size)
    }

    #[inline]
    pub fn assert_uint(self, size: Size) -> u128 {
        self.assert_bits(size)
    }

    // Tries to convert the `ScalarInt` to `u8`. Fails if the `size` of the `ScalarInt`
    // in not equal to 1 byte and returns the `size` value of the `ScalarInt` in
    // that case.
    #[inline]
    pub fn try_to_u8(self) -> Result<u8, Size> {
        self.try_to_uint(Size::from_bits(8)).map(|v| u8::try_from(v).unwrap())
    }

    /// Tries to convert the `ScalarInt` to `u16`. Fails if the size of the `ScalarInt`
    /// in not equal to 2 bytes and returns the `size` value of the `ScalarInt` in
    /// that case.
    #[inline]
    pub fn try_to_u16(self) -> Result<u16, Size> {
        self.try_to_uint(Size::from_bits(16)).map(|v| u16::try_from(v).unwrap())
    }

    /// Tries to convert the `ScalarInt` to `u32`. Fails if the `size` of the `ScalarInt`
    /// in not equal to 4 bytes and returns the `size` value of the `ScalarInt` in
    /// that case.
    #[inline]
    pub fn try_to_u32(self) -> Result<u32, Size> {
        self.try_to_uint(Size::from_bits(32)).map(|v| u32::try_from(v).unwrap())
    }

    /// Tries to convert the `ScalarInt` to `u64`. Fails if the `size` of the `ScalarInt`
    /// in not equal to 8 bytes and returns the `size` value of the `ScalarInt` in
    /// that case.
    #[inline]
    pub fn try_to_u64(self) -> Result<u64, Size> {
        self.try_to_uint(Size::from_bits(64)).map(|v| u64::try_from(v).unwrap())
    }

    /// Tries to convert the `ScalarInt` to `u128`. Fails if the `size` of the `ScalarInt`
    /// in not equal to 16 bytes and returns the `size` value of the `ScalarInt` in
    /// that case.
    #[inline]
    pub fn try_to_u128(self) -> Result<u128, Size> {
        self.try_to_uint(Size::from_bits(128))
    }

    #[inline]
    pub fn try_to_target_usize(&self, tcx: TyCtxt<'_>) -> Result<u64, Size> {
        self.try_to_uint(tcx.data_layout.pointer_size).map(|v| u64::try_from(v).unwrap())
    }

    // Tries to convert the `ScalarInt` to `bool`. Fails if the `size` of the `ScalarInt`
    // in not equal to 1 byte or if the value is not 0 or 1 and returns the `size`
    // value of the `ScalarInt` in that case.
    #[inline]
    pub fn try_to_bool(self) -> Result<bool, Size> {
        match self.try_to_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(self.size()),
        }
    }

    /// Tries to convert the `ScalarInt` to a signed integer of the given size.
    /// Fails if the size of the `ScalarInt` is not equal to `size` and returns the
    /// `ScalarInt`s size in that case.
    #[inline]
    pub fn try_to_int(self, size: Size) -> Result<i128, Size> {
        let b = self.try_to_bits(size)?;
        Ok(size.sign_extend(b) as i128)
    }

    #[inline]
    pub fn assert_int(self, size: Size) -> i128 {
        let b = self.assert_bits(size);
        size.sign_extend(b) as i128
    }

    /// Tries to convert the `ScalarInt` to i8.
    /// Fails if the size of the `ScalarInt` is not equal to 1 byte
    /// and returns the `ScalarInt`s size in that case.
    pub fn try_to_i8(self) -> Result<i8, Size> {
        self.try_to_int(Size::from_bits(8)).map(|v| i8::try_from(v).unwrap())
    }

    /// Tries to convert the `ScalarInt` to i16.
    /// Fails if the size of the `ScalarInt` is not equal to 2 bytes
    /// and returns the `ScalarInt`s size in that case.
    pub fn try_to_i16(self) -> Result<i16, Size> {
        self.try_to_int(Size::from_bits(16)).map(|v| i16::try_from(v).unwrap())
    }

    /// Tries to convert the `ScalarInt` to i32.
    /// Fails if the size of the `ScalarInt` is not equal to 4 bytes
    /// and returns the `ScalarInt`s size in that case.
    pub fn try_to_i32(self) -> Result<i32, Size> {
        self.try_to_int(Size::from_bits(32)).map(|v| i32::try_from(v).unwrap())
    }

    /// Tries to convert the `ScalarInt` to i64.
    /// Fails if the size of the `ScalarInt` is not equal to 8 bytes
    /// and returns the `ScalarInt`s size in that case.
    pub fn try_to_i64(self) -> Result<i64, Size> {
        self.try_to_int(Size::from_bits(64)).map(|v| i64::try_from(v).unwrap())
    }

    /// Tries to convert the `ScalarInt` to i128.
    /// Fails if the size of the `ScalarInt` is not equal to 16 bytes
    /// and returns the `ScalarInt`s size in that case.
    pub fn try_to_i128(self) -> Result<i128, Size> {
        self.try_to_int(Size::from_bits(128))
    }

    #[inline]
    pub fn try_to_target_isize(&self, tcx: TyCtxt<'_>) -> Result<i64, Size> {
        self.try_to_int(tcx.data_layout.pointer_size).map(|v| i64::try_from(v).unwrap())
    }

    #[inline]
    pub fn try_to_float<F: Float>(self) -> Result<F, Size> {
        // Going through `to_uint` to check size and truncation.
        Ok(F::from_bits(self.try_to_bits(Size::from_bits(F::BITS))?))
    }

    #[inline]
    pub fn try_to_f16(self) -> Result<Half, Size> {
        self.try_to_float()
    }

    #[inline]
    pub fn try_to_f32(self) -> Result<Single, Size> {
        self.try_to_float()
    }

    #[inline]
    pub fn try_to_f64(self) -> Result<Double, Size> {
        self.try_to_float()
    }

    #[inline]
    pub fn try_to_f128(self) -> Result<Quad, Size> {
        self.try_to_float()
    }
}

macro_rules! from {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for ScalarInt {
                #[inline]
                fn from(u: $ty) -> Self {
                    Self {
                        data: u128::from(u),
                        size: NonZero::new(std::mem::size_of::<$ty>() as u8).unwrap(),
                    }
                }
            }
        )*
    }
}

macro_rules! try_from {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<ScalarInt> for $ty {
                type Error = Size;
                #[inline]
                fn try_from(int: ScalarInt) -> Result<Self, Size> {
                    // The `unwrap` cannot fail because to_bits (if it succeeds)
                    // is guaranteed to return a value that fits into the size.
                    int.try_to_bits(Size::from_bytes(std::mem::size_of::<$ty>()))
                       .map(|u| u.try_into().unwrap())
                }
            }
        )*
    }
}

from!(u8, u16, u32, u64, u128, bool);
try_from!(u8, u16, u32, u64, u128);

impl TryFrom<ScalarInt> for bool {
    type Error = Size;
    #[inline]
    fn try_from(int: ScalarInt) -> Result<Self, Size> {
        int.try_to_bool()
    }
}

impl From<char> for ScalarInt {
    #[inline]
    fn from(c: char) -> Self {
        Self { data: c as u128, size: NonZero::new(std::mem::size_of::<char>() as u8).unwrap() }
    }
}

/// Error returned when a conversion from ScalarInt to char fails.
#[derive(Debug)]
pub struct CharTryFromScalarInt;

impl TryFrom<ScalarInt> for char {
    type Error = CharTryFromScalarInt;

    #[inline]
    fn try_from(int: ScalarInt) -> Result<Self, Self::Error> {
        let Ok(bits) = int.try_to_bits(Size::from_bytes(std::mem::size_of::<char>())) else {
            return Err(CharTryFromScalarInt);
        };
        match char::from_u32(bits.try_into().unwrap()) {
            Some(c) => Ok(c),
            None => Err(CharTryFromScalarInt),
        }
    }
}

impl From<Half> for ScalarInt {
    #[inline]
    fn from(f: Half) -> Self {
        // We trust apfloat to give us properly truncated data.
        Self { data: f.to_bits(), size: NonZero::new((Half::BITS / 8) as u8).unwrap() }
    }
}

impl TryFrom<ScalarInt> for Half {
    type Error = Size;
    #[inline]
    fn try_from(int: ScalarInt) -> Result<Self, Size> {
        int.try_to_bits(Size::from_bytes(2)).map(Self::from_bits)
    }
}

impl From<Single> for ScalarInt {
    #[inline]
    fn from(f: Single) -> Self {
        // We trust apfloat to give us properly truncated data.
        Self { data: f.to_bits(), size: NonZero::new((Single::BITS / 8) as u8).unwrap() }
    }
}

impl TryFrom<ScalarInt> for Single {
    type Error = Size;
    #[inline]
    fn try_from(int: ScalarInt) -> Result<Self, Size> {
        int.try_to_bits(Size::from_bytes(4)).map(Self::from_bits)
    }
}

impl From<Double> for ScalarInt {
    #[inline]
    fn from(f: Double) -> Self {
        // We trust apfloat to give us properly truncated data.
        Self { data: f.to_bits(), size: NonZero::new((Double::BITS / 8) as u8).unwrap() }
    }
}

impl TryFrom<ScalarInt> for Double {
    type Error = Size;
    #[inline]
    fn try_from(int: ScalarInt) -> Result<Self, Size> {
        int.try_to_bits(Size::from_bytes(8)).map(Self::from_bits)
    }
}

impl From<Quad> for ScalarInt {
    #[inline]
    fn from(f: Quad) -> Self {
        // We trust apfloat to give us properly truncated data.
        Self { data: f.to_bits(), size: NonZero::new((Quad::BITS / 8) as u8).unwrap() }
    }
}

impl TryFrom<ScalarInt> for Quad {
    type Error = Size;
    #[inline]
    fn try_from(int: ScalarInt) -> Result<Self, Size> {
        int.try_to_bits(Size::from_bytes(16)).map(Self::from_bits)
    }
}

impl fmt::Debug for ScalarInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Dispatch to LowerHex below.
        write!(f, "0x{self:x}")
    }
}

impl fmt::LowerHex for ScalarInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.check_data();
        if f.alternate() {
            // Like regular ints, alternate flag adds leading `0x`.
            write!(f, "0x")?;
        }
        // Format as hex number wide enough to fit any value of the given `size`.
        // So data=20, size=1 will be "0x14", but with size=4 it'll be "0x00000014".
        // Using a block `{self.data}` here to force a copy instead of using `self.data`
        // directly, because `write!` takes references to its formatting arguments and
        // would thus borrow `self.data`. Since `Self`
        // is a packed struct, that would create a possibly unaligned reference, which
        // is UB.
        write!(f, "{:01$x}", { self.data }, self.size.get() as usize * 2)
    }
}

impl fmt::UpperHex for ScalarInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.check_data();
        // Format as hex number wide enough to fit any value of the given `size`.
        // So data=20, size=1 will be "0x14", but with size=4 it'll be "0x00000014".
        // Using a block `{self.data}` here to force a copy instead of using `self.data`
        // directly, because `write!` takes references to its formatting arguments and
        // would thus borrow `self.data`. Since `Self`
        // is a packed struct, that would create a possibly unaligned reference, which
        // is UB.
        write!(f, "{:01$X}", { self.data }, self.size.get() as usize * 2)
    }
}

impl fmt::Display for ScalarInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.check_data();
        write!(f, "{}", { self.data })
    }
}

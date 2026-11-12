//! Receivers of method calls whose type the file tells: fields, `&Self`, macro bodies.

pub struct Ident {}

pub struct Meta {
    id: Ident,
    #[cfg(feature = "std")]
    extra: Ident,
    #[cfg(not(feature = "std"))]
    extra: Other,
}

pub struct Other {}

pub struct Pair(Ident, u8);

pub struct View<'a> {
    meta: &'a mut Meta,
    count: usize,
}

impl Meta {
    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }
    pub fn cmp_with(&self, rhs: &Self) -> bool {
        rhs.as_str() == self.as_str()
    }
    pub fn mixed(&self) -> bool {
        // cfg variants disagree on `extra`: unknown
        self.extra.is_empty()
    }
}

impl Pair {
    pub fn first_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'a> View<'a> {
    pub fn deep(&self) -> bool {
        self.meta.id.is_empty() && self.count.is_power_of_two()
    }
    pub fn param(&self, other: &Meta) -> bool {
        other.id.is_empty()
    }
}

#[macro_export]
macro_rules! meta {
    () => {
        $crate::Meta::build()
    };
    ($x:expr) => {{
        let m = $crate::Meta::from_id($x);
        m
    }};
}

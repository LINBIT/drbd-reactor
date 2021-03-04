/// A trait for matching structs from "partial" filters.
///
/// This is meant as a very basic generic filter for a bunch of different structs or enums.
/// Structs are compared field by field. The "pattern" is a struct/enum with the same fields or
/// enums, but everything made optional.
pub trait PartialMatchable {
    /// The pattern type for comparison to Self. An all-values-optional sibling of Self.
    type Pattern;
    /// Test is self matches the given pattern.
    fn matches(&self, matcher: &Self::Pattern) -> bool;
}

#[macro_export]
macro_rules! common_matchable {
    ($($ty:ty),*) => {
        $(
            impl $crate::matchable::PartialMatchable for $ty {
                type Pattern = ::core::option::Option<$ty>;
                fn matches(&self, pattern: &Self::Pattern) -> bool {
                    match pattern {
                        Some(s) => self == s,
                        None => true,
                    }
                }
            }
        )*
    };
}

// The generic impls for common types (used in the crate)
common_matchable![String, bool, i32, u64];

/// Implement PartialMatchable for structs and (unit) enums
///
/// Creates an "all optional" copy of the given struct or enum and implements the
/// PartialMatchable trait for the original.
///
/// # Example
///```rust
/// use drbdd::make_matchable;
/// use drbdd::matchable::PartialMatchable;
///
/// make_matchable!(struct Foo { item: String }, FooPattern);
///
/// let foo_pattern = Some(FooPattern { item: Some("a".to_string()) });
/// let a = Foo { item: "a".to_string() };
/// let b = Foo { item: "b".to_string() };
/// assert!(a.matches(&None));
/// assert!(a.matches(&foo_pattern));
/// assert!(!b.matches(&foo_pattern));
///
/// make_matchable!(enum Bar { A, B });
/// let bar_pattern = Some(Bar::A);
/// let a = Bar::A;
/// let b = Bar::B;
/// assert!(a.matches(&None));
/// assert!(a.matches(&bar_pattern));
/// assert!(!b.matches(&bar_pattern));
///```
#[macro_export]
macro_rules! make_matchable {
    ($(#[$structattr:meta])* $svis:vis struct $name:ident { $($(#[$fieldattr:meta])* $fvis:vis $field:ident: $field_ty:ty),* $(,)? }, $pattern:ident) => {
        $(
            #[$structattr]
        )*
        $svis struct $name {
        $(
            $(
                #[$fieldattr]
            )*
            $fvis $field: $field_ty,
        )*
        }

        $(
            #[$structattr]
        )*
        $svis struct $pattern {
        $(
            $(
                #[$fieldattr]
            )*
            $fvis $field: <$field_ty as $crate::matchable::PartialMatchable>::Pattern,
        )*
        }

        impl $crate::matchable::PartialMatchable for $name {
            type Pattern = ::core::option::Option<$pattern>;
            fn matches(&self, pattern: &Self::Pattern) -> bool {
                let pattern = match pattern.as_ref() {
                    Some(v) => v,
                    None => return true,
                };
                $(
                    if !$crate::matchable::PartialMatchable::matches(&self.$field, &pattern.$field) {
                        return false
                    }
                )*
                true
            }
        }
    };

    // Matches basic enums: unit variants should work, tuple and struct variant won't
    // The pattern type is Option<Enum>
    ($(#[$enumattr:meta])* $evis:vis enum $name:ident { $($(#[$variantattr:meta])* $variant:ident),* $(,)? }) => {
        $(
            #[$enumattr]
        )*
        $evis enum $name {
        $(
            $(
                #[$variantattr]
            )*
            $variant,
        )*
        }

        impl $crate::matchable::PartialMatchable for $name {
            type Pattern = ::core::option::Option<$name>;
            fn matches(&self, pattern: &Self::Pattern) -> bool {
                match (self, pattern) {
                $(
                    ($name::$variant, ::core::option::Option::Some($name::$variant)) => true,
                )*
                    (_, ::core::option::Option::None) => true,
                    _ => false,
                }
            }
        }
    };
}

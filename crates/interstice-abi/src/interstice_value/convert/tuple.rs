use crate::{IntersticeAbiError, IntersticeValue};

macro_rules! impl_tuple_into_interstice {
    ( $( $name:ident ),+ ) => {
        impl<$( $name ),+> From<( $( $name ),+ )> for IntersticeValue
        where
            $( $name: Into<IntersticeValue> ),+
        {
            fn from(value: ( $( $name ),+ )) -> Self {
                let ( $( $name ),+ ) = value;
                IntersticeValue::Tuple(vec![
                    $( $name.into() ),+
                ])
            }
        }
    };
}

impl_tuple_into_interstice!(A, B);
impl_tuple_into_interstice!(A, B, C);
impl_tuple_into_interstice!(A, B, C, D);
impl_tuple_into_interstice!(A, B, C, D, E);
impl_tuple_into_interstice!(A, B, C, D, E, F);
impl_tuple_into_interstice!(A, B, C, D, E, F, G);
impl_tuple_into_interstice!(A, B, C, D, E, F, G, H);

macro_rules! count_idents {
    ($($idents:ident),*) => {
        <[()]>::len(&[$(count_idents!(@sub $idents)),*])
    };
    (@sub $ident:ident) => { () };
}

macro_rules! impl_tuple_tryfrom_interstice {
    ( $( $name:ident ),+ ) => {
        impl<$( $name ),+> TryFrom<IntersticeValue> for ( $( $name ),+ )
        where
            $( $name: TryFrom<IntersticeValue> ),+,
        {
            type Error = IntersticeAbiError;

            fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
                match value {
                    IntersticeValue::Tuple(vec) => {
                        let expected = count_idents!( $( $name ),+ );
                        if vec.len() != expected {
                            return Err(IntersticeAbiError::ConversionError(format!(
                                "Tuple arity mismatch: expected {}, got {}",
                                expected,
                                vec.len()
                            )));
                        }

                        // We index instead of consuming iterator so order is explicit
                        let mut iter = vec.into_iter();
                        Ok((
                            $(
                                {
                                    let v = iter.next().unwrap();
                                    <$name  as TryFrom<IntersticeValue>>::try_from(v).map_err(|err| {IntersticeAbiError::ConversionError(format!("Couldn't convert inner tuple value"))})?
                                }
                            ),+
                        ))
                    }
                    other => Err(IntersticeAbiError::ConversionError(format!("Expected Tuple, got {:?}", other))),
                }
            }
        }
    };
}

impl_tuple_tryfrom_interstice!(A, B);
impl_tuple_tryfrom_interstice!(A, B, C);
impl_tuple_tryfrom_interstice!(A, B, C, D);
impl_tuple_tryfrom_interstice!(A, B, C, D, E);
impl_tuple_tryfrom_interstice!(A, B, C, D, E, F);
impl_tuple_tryfrom_interstice!(A, B, C, D, E, F, G);
impl_tuple_tryfrom_interstice!(A, B, C, D, E, F, G, H);

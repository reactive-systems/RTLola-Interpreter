use convert_case::{Case, Casing};
use proc_macro2::Ident;
use quote::format_ident;
// macro_rules! factories_tuple_impls {
//     ($num:expr, $( $name:ident )+ ) => {
//         paste!{
//             #[doc=concat!("Implements [EventFactory] for a ", stringify!($num), "-tuple of types that implement [AssociatedFactory]")]
//             #[allow(missing_debug_implementations)]
//             pub struct [<TupleFactory $num>]<
//                 $([<$name I>]: EventFactory<CreationData = (), Record = $name>),+,
//                 $($name: AssociatedFactory<Input = [<$name I>]> + Send),+
//             > {
//                 $([<$name:lower>]: [<$name I>]),+
//             }
//
//             impl<
//                 $([<$name I>]: EventFactory<CreationData = (), Record = $name>),+,
//                 $($name: AssociatedFactory<Input = [<$name I>]> + Send),+
//                 > Input for[<TupleInput $num>]<$([<$name I>]),+,$($name),+>
//             {
//                 type CreationData = ();
//                 type Error = EventFactoryError;
//                 type Record = ($($name),+);
//
//                 fn try_new(
//                     map: HashMap<String, InputReference>,
//                     _setup_data: Self::CreationData,
//                 ) -> Result<(Self, Vec<String>), EventFactoryError> {
//                     let mut total_found: HashSet<String> = map.keys().cloned().collect();
//                     $(
//                         let ([<$name:lower>], found) = [<$name I>]::try_new(map.clone(), ())?;
//                         total_found.extend(found);
//                     )+
//                     Ok((Self { $([<$name:lower>]),+ }, total_found.into_iter().collect()))
//                 }
//
//                 fn get_event(&self, rec: Self::Record) -> Result<Event, EventFactoryError> {
//                     let ($([<$name:lower _rec>]),+) = rec;
//                     $(
//                         let [<$name:lower _event>] = self.[<$name:lower>].get_event([<$name:lower _rec>])?;
//                     )+
//                     Ok(izip!($([<$name:lower _event>]),+).map(|($([<$name:lower>]),+)| Value::None$(.and_then([<$name:lower>]))+).collect())
//                 }
//             }
//
//             impl<
//                 $([<$name I>]: EventFactory<CreationData = (), Record = $name>),+,
//                 $($name: AssociatedFactory<Input = [<$name I>]> + Send),+
//                 > AssociatedFactory for ($($name),+) {
//                 type Input = [<TupleFactory $num>]<
//                                 $([<$name I>]),+,
//                                 $($name),+
//                             > ;
//             }
//         }
//     };
// }

pub(crate) fn new_snake_ident(base: &Ident, suffix: &str) -> Ident {
    let ident_str = base.to_string().to_case(Case::Snake);
    format_ident!("{}_{}", ident_str, suffix, span = base.span())
}

use crate::exports::*;
use devise::ext::SpanDiagnosticExt;
use devise::Support;
use devise::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt;

pub fn derive_from_param(input: proc_macro::TokenStream) -> TokenStream {
    DeriveGenerator::build_for(input, quote!(impl<'a> #_request::FromParam<'a>))
        .support(Support::Enum)
        .validator(ValidatorBuild::new().fields_validate(|_, fields| {
            if !fields.is_empty() {
                return Err(fields
                    .span()
                    .error("Only enums without data fields are supported"));
            }
            Ok(())
        }))
        .inner_mapper(MapperBuild::new().enum_map(|_, data| {
            let matches = data.variants().map(|field| {
                let field_name = field.ident.unraw();
                quote!(
                    stringify!(#field_name) => Ok(Self::#field),
                )
            });
            let names = data.variants().map(|field| {
                let field_name = field.ident.unraw();
                quote!(
                    #_Cow::Borrowed(stringify!(#field_name)),
                )
            });

            quote! {
                type Error = #_request::EnumFromParamError<'a>;
                fn from_param(param: &'a str) -> Result<Self, Self::Error> {
                    match param {
                        #(#matches)*
                        _ => Err(#_request::EnumFromParamError::new(
                            #_Cow::Borrowed(param),
                            #_Cow::Borrowed(&[#(#names)*]),
                        )),
                    }
                }
            }
        }))
        .to_tokens()
}

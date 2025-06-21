//! Macros to make defining MCP servers easier
//!
//! Using the #[mcp_server] macro on an impl block, this crate will pick up all methods
//! marked with #[tool] and derive the necessary ServerConn::call_tool, ServerConn::list_tools and
//! ServerConn::initialize methods. The name of the server is derived from the name of the struct,
//! and the description is derived from the doc comment on the impl block. The version is set to
//! "0.1.0" by default.
//!
//! All tool methods have the following exact signature:  
//!
//! ```ignore
//! async fn tool_name(&self, context: &ServerCtx, params: ToolParams) -> Result<schema::CallToolResult>
//! ```
//!
//! The parameter struct (ToolParams in this example) must implement `schemars::JsonSchema` and
//! `serde::Deserialize`.
//!
//! Example usage:
//!
//! ```ignore
//! use tenx_mcp::{ServerCtx, schema};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
//! struct EchoParams {
//!     /// The message to echo back
//!     message: String,
//! }
//!
//! /// Basic server connection that provides an echo tool
//! #[derive(Debug, Default)]
//! struct Basic {}
//!
//! #[mcp_server]
//! /// This is the description field for the server.
//! impl Basic {
//!     #[tool]
//!     async fn echo(
//!         &self,
//!         context: &ServerCtx,
//!         params: EchoParams,
//!     ) -> Result<schema::CallToolResult> {
//!         Ok(schema::CallToolResult::new().with_text_content(params.message))
//!     }
//! }
//! ```

use proc_macro2::TokenStream;
use quote::quote;
use syn::{spanned::Spanned, Expr, ExprLit, ImplItem, ItemImpl, Lit, Meta};

type Result<T> = std::result::Result<T, syn::Error>;

#[derive(Debug)]
struct ToolMethod {
    name: String,
    docs: String,
    params_type: syn::Type,
}

#[derive(Debug)]
struct ServerInfo {
    struct_name: String,
    description: String,
    tools: Vec<ToolMethod>,
}

fn extract_doc_comment(attrs: &[syn::Attribute]) -> String {
    let mut docs = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let Meta::NameValue(meta) = &attr.meta {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = &meta.value
                {
                    let doc = s.value();
                    let doc = doc.trim();
                    if !doc.is_empty() {
                        docs.push(doc.to_string());
                    }
                }
            }
        }
    }
    docs.join("\n")
}

fn parse_tool_method(method: &syn::ImplItemFn) -> Result<Option<ToolMethod>> {
    let has_tool_attr = method.attrs.iter().any(|attr| attr.path().is_ident("tool"));

    if !has_tool_attr {
        return Ok(None);
    }

    let name = method.sig.ident.to_string();
    let docs = extract_doc_comment(&method.attrs);

    // Validate method signature
    if method.sig.asyncness.is_none() {
        return Err(syn::Error::new(
            method.sig.span(),
            "tool methods must be async",
        ));
    }

    // Check parameters
    let params: Vec<_> = method.sig.inputs.iter().collect();
    if params.len() != 3 {
        return Err(syn::Error::new(
            method.sig.inputs.span(),
            "tool methods must have exactly 3 parameters: &self, context: &ServerCtx, params: ParamsType",
        ));
    }

    // Validate &self or &mut self
    match params[0] {
        syn::FnArg::Receiver(_r) => {
            // Accept both &self and &mut self
        }
        _ => {
            return Err(syn::Error::new(
                params[0].span(),
                "first parameter must be &self or &mut self",
            ));
        }
    }

    // Extract params type (third parameter)
    let params_type = match params[2] {
        syn::FnArg::Typed(pat_type) => (*pat_type.ty).clone(),
        _ => {
            return Err(syn::Error::new(
                params[2].span(),
                "third parameter must be a typed parameter",
            ));
        }
    };

    // Validate return type
    match &method.sig.output {
        syn::ReturnType::Type(_, _ty) => {
            // We just check it exists, actual validation would be more complex
        }
        _ => {
            return Err(syn::Error::new(
                method.sig.output.span(),
                "tool methods must return Result<schema::CallToolResult>",
            ));
        }
    }

    Ok(Some(ToolMethod {
        name,
        docs,
        params_type,
    }))
}

fn parse_impl_block(input: &TokenStream) -> Result<(ItemImpl, ServerInfo)> {
    let impl_block = syn::parse2::<ItemImpl>(input.clone())?;

    // Extract struct name
    let struct_name = match &*impl_block.self_ty {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .ok_or_else(|| syn::Error::new(impl_block.self_ty.span(), "Invalid type name"))?
            .ident
            .to_string(),
        _ => {
            return Err(syn::Error::new(
                impl_block.self_ty.span(),
                "Expected a struct or type name",
            ))
        }
    };

    // Extract description from doc comment
    let description = extract_doc_comment(&impl_block.attrs);

    // Extract tool methods
    let mut tools = Vec::new();
    for item in &impl_block.items {
        if let ImplItem::Fn(method) = item {
            if let Some(tool) = parse_tool_method(method)? {
                tools.push(tool);
            }
        }
    }

    Ok((
        impl_block,
        ServerInfo {
            struct_name,
            description,
            tools,
        },
    ))
}

fn generate_call_tool(info: &ServerInfo) -> TokenStream {
    let tool_matches = info.tools.iter().map(|tool| {
        let name = &tool.name;
        let method = syn::Ident::new(name, proc_macro2::Span::call_site());
        let params_type = &tool.params_type;

        quote! {
            #name => {
                let args = arguments.ok_or_else(||
                    tenx_mcp::Error::InvalidParams("Missing arguments".to_string())
                )?;
                let params: #params_type = serde_json::from_value(serde_json::Value::Object(args.into_iter().collect()))
                    .map_err(|e| tenx_mcp::Error::InvalidParams(e.to_string()))?;
                self.#method(context, params).await
            }
        }
    });

    quote! {
        async fn call_tool(
            &self,
            context: &tenx_mcp::ServerCtx,
            name: String,
            arguments: Option<std::collections::HashMap<String, serde_json::Value>>,
        ) -> tenx_mcp::Result<tenx_mcp::schema::CallToolResult> {
            match name.as_str() {
                #(#tool_matches)*
                _ => Err(tenx_mcp::Error::MethodNotFound(format!("Unknown tool: {}", name)))
            }
        }
    }
}

fn generate_list_tools(info: &ServerInfo) -> TokenStream {
    let tools = info.tools.iter().map(|tool| {
        let name = &tool.name;
        let description = &tool.docs;
        let params_type = &tool.params_type;

        quote! {
            {
                tenx_mcp::schema::Tool::new(#name, tenx_mcp::schema::ToolInputSchema::from_json_schema::<#params_type>())
                    .with_description(#description)
            }
        }
    });

    quote! {
        async fn list_tools(
            &self,
            _context: &tenx_mcp::ServerCtx,
            _cursor: Option<tenx_mcp::schema::Cursor>,
        ) -> tenx_mcp::Result<tenx_mcp::schema::ListToolsResult> {
            Ok(tenx_mcp::schema::ListToolsResult {
                tools: vec![#(#tools),*],
                next_cursor: None,
            })
        }
    }
}

fn generate_initialize(info: &ServerInfo) -> TokenStream {
    let name = &info.struct_name;
    let description = &info.description;

    quote! {
        async fn initialize(
            &self,
            _context: &tenx_mcp::ServerCtx,
            _protocol_version: String,
            _capabilities: tenx_mcp::schema::ClientCapabilities,
            _client_info: tenx_mcp::schema::Implementation,
        ) -> tenx_mcp::Result<tenx_mcp::schema::InitializeResult> {
            Ok(tenx_mcp::schema::InitializeResult {
                protocol_version: "1.0.0".to_string(),
                capabilities: tenx_mcp::schema::ServerCapabilities {
                    tools: Some(tenx_mcp::schema::ToolsCapability {
                        list_changed: Some(false),
                    }),
                    ..Default::default()
                },
                server_info: tenx_mcp::schema::Implementation {
                    name: #name.to_string(),
                    version: "0.1.0".to_string(),
                },
                instructions: Some(#description.to_string()),
                meta: None,
            })
        }
    }
}

fn inner_mcp_server(_attr: TokenStream, input: TokenStream) -> Result<TokenStream> {
    let (impl_block, info) = parse_impl_block(&input)?;

    if info.tools.is_empty() {
        return Err(syn::Error::new(
            input.span(),
            "No tool methods found. Use #[tool] to mark methods as MCP tools",
        ));
    }

    let struct_name = syn::Ident::new(&info.struct_name, proc_macro2::Span::call_site());
    let call_tool = generate_call_tool(&info);
    let list_tools = generate_list_tools(&info);
    let initialize = generate_initialize(&info);

    Ok(quote! {
        #impl_block

        #[async_trait::async_trait]
        impl tenx_mcp::ServerConn for #struct_name {
            #initialize
            #list_tools
            #call_tool
        }
    })
}

/// Derive the ServerConn methods from an impl block.
#[proc_macro_attribute]
pub fn mcp_server(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attr_tokens = TokenStream::from(attr);
    let input_tokens = TokenStream::from(input.clone());

    match inner_mcp_server(attr_tokens, input_tokens) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// Mark a method as an mcp tool.
#[proc_macro_attribute]
pub fn tool(
    _attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_doc_extraction() {
        let attrs = vec![
            syn::parse_quote! { #[doc = " First line"] },
            syn::parse_quote! { #[doc = " Second line"] },
            syn::parse_quote! { #[doc = ""] },
            syn::parse_quote! { #[doc = " Third line"] },
        ];

        let result = extract_doc_comment(&attrs);
        assert_eq!(result, "First line\nSecond line\nThird line");
    }

    #[test]
    fn test_parse_tool_method_valid() {
        let method: syn::ImplItemFn = syn::parse_quote! {
            #[tool]
            /// This is a test tool
            async fn test_tool(&self, context: &ServerCtx, params: TestParams) -> Result<schema::CallToolResult> {
                Ok(schema::CallToolResult::new())
            }
        };

        let result = parse_tool_method(&method).unwrap();
        assert!(result.is_some());

        let tool = result.unwrap();
        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.docs, "This is a test tool");
    }

    #[test]
    fn test_parse_tool_method_not_async() {
        let method: syn::ImplItemFn = syn::parse_quote! {
            #[tool]
            fn test_tool(&mut self, context: &ServerCtx, params: TestParams) -> Result<schema::CallToolResult> {
                Ok(schema::CallToolResult::new())
            }
        };

        let result = parse_tool_method(&method);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be async"));
    }

    #[test]
    fn test_parse_tool_method_wrong_params() {
        let method: syn::ImplItemFn = syn::parse_quote! {
            #[tool]
            async fn test_tool(&mut self) -> Result<schema::CallToolResult> {
                Ok(schema::CallToolResult::new())
            }
        };

        let result = parse_tool_method(&method);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exactly 3 parameters"));
    }

    #[test]
    fn test_parse_tool_method_both_self_types() {
        // Test that both &self and &mut self are accepted
        let method1: syn::ImplItemFn = syn::parse_quote! {
            #[tool]
            async fn test_tool(&self, context: &ServerCtx, params: TestParams) -> Result<schema::CallToolResult> {
                Ok(schema::CallToolResult::new())
            }
        };
        assert!(parse_tool_method(&method1).unwrap().is_some());

        let method2: syn::ImplItemFn = syn::parse_quote! {
            #[tool]
            async fn test_tool(&mut self, context: &ServerCtx, params: TestParams) -> Result<schema::CallToolResult> {
                Ok(schema::CallToolResult::new())
            }
        };
        assert!(parse_tool_method(&method2).unwrap().is_some());
    }

    #[test]
    fn test_parse_impl_block() {
        let input = quote! {
            /// Test server implementation
            impl TestServer {
                #[tool]
                /// Echo tool
                async fn echo(&self, context: &ServerCtx, params: EchoParams) -> Result<schema::CallToolResult> {
                    Ok(schema::CallToolResult::new())
                }

                #[tool]
                async fn ping(&self, context: &ServerCtx, params: PingParams) -> Result<schema::CallToolResult> {
                    Ok(schema::CallToolResult::new())
                }

                // This method should be ignored
                async fn helper(&self) -> String {
                    "helper".to_string()
                }
            }
        };

        let (_, info) = parse_impl_block(&input).unwrap();
        assert_eq!(info.struct_name, "TestServer");
        assert_eq!(info.description, "Test server implementation");
        assert_eq!(info.tools.len(), 2);
        assert_eq!(info.tools[0].name, "echo");
        assert_eq!(info.tools[0].docs, "Echo tool");
        assert_eq!(info.tools[1].name, "ping");
    }

    #[test]
    fn test_generate_call_tool() {
        let info = ServerInfo {
            struct_name: "TestServer".to_string(),
            description: "Test description".to_string(),
            tools: vec![
                ToolMethod {
                    name: "echo".to_string(),
                    docs: "Echo tool".to_string(),
                    params_type: syn::parse_quote! { EchoParams },
                },
                ToolMethod {
                    name: "ping".to_string(),
                    docs: "Ping tool".to_string(),
                    params_type: syn::parse_quote! { PingParams },
                },
            ],
        };

        let generated = generate_call_tool(&info);
        let generated_str = generated.to_string();

        assert!(generated_str.contains("call_tool"));
        assert!(generated_str.contains("match name . as_str ()"));
        assert!(generated_str.contains("\"echo\" =>"));
        assert!(generated_str.contains("\"ping\" =>"));
        assert!(generated_str.contains("self . echo (context , params) . await"));
        assert!(generated_str.contains("self . ping (context , params) . await"));
    }

    #[test]
    fn test_generate_list_tools() {
        let info = ServerInfo {
            struct_name: "TestServer".to_string(),
            description: "Test description".to_string(),
            tools: vec![ToolMethod {
                name: "echo".to_string(),
                docs: "Echo tool".to_string(),
                params_type: syn::parse_quote! { EchoParams },
            }],
        };

        let generated = generate_list_tools(&info);
        let generated_str = generated.to_string();

        assert!(generated_str.contains("list_tools"));
        assert!(generated_str.contains("Tool :: new"));
        assert!(generated_str.contains("with_description"));
        assert!(generated_str.contains("\"echo\""));
        assert!(generated_str.contains("\"Echo tool\""));
    }

    #[test]
    fn test_full_macro_expansion() {
        let input = quote! {
            /// Test server
            impl TestServer {
                #[tool]
                /// Echo back the input
                async fn echo(&self, context: &ServerCtx, params: EchoParams) -> Result<schema::CallToolResult> {
                    Ok(schema::CallToolResult::new())
                }
            }
        };

        let result = inner_mcp_server(TokenStream::new(), input).unwrap();
        let result_str = result.to_string();

        // Check that original impl block is preserved
        assert!(result_str.contains("impl TestServer"));
        assert!(result_str.contains("async fn echo"));

        // Check that ServerConn impl is generated
        assert!(result_str.contains("impl tenx_mcp :: ServerConn for TestServer"));
        assert!(result_str.contains("async fn initialize"));
        assert!(result_str.contains("async fn list_tools"));
        assert!(result_str.contains("async fn call_tool"));
    }

    #[test]
    fn test_no_tools_error() {
        let input = quote! {
            impl TestServer {
                async fn helper(&self) -> String {
                    "helper".to_string()
                }
            }
        };

        let result = inner_mcp_server(TokenStream::new(), input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No tool methods found"));
    }
}

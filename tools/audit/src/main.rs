use quote::quote;
use std::{collections::BTreeMap, env, fs, path::Path};
use syn::{Block, Item};

fn block<'a>(items: &'a mut [Item], node: &[usize]) -> &'a mut Block {
    let (index, rest) = node.split_first().unwrap();
    match &mut items[*index] {
        Item::Fn(f) if rest.is_empty() => &mut f.block,
        Item::Impl(i) if rest.len() == 1 => match &mut i.items[rest[0]] {
            syn::ImplItem::Fn(f) => &mut f.block,
            _ => panic!("planned method absent"),
        },
        Item::Mod(m) => block(&mut m.content.as_mut().unwrap().1, rest),
        _ => panic!("planned target absent"),
    }
}

fn canonical(items: &[Item]) -> String {
    prettyplease::unparse(&syn::File {
        shebang: None,
        attrs: vec![],
        items: items.to_vec(),
    })
}

/// Unwrap a RETURN injection. The staged block's last statement is
/// `({ fn helper(..) {..} helper })(orig_args)` — a parenthesized block
/// expression (helper definition + bare fn path) immediately called with
/// the original arguments, where the first argument is the original tail
/// expression (bound to the "$value" parameter). Verifies the helper
/// definition and the non-$value planned bindings against the call, then
/// returns the original tail expression.
fn pop_return_wrapper(
    b: &mut Block,
    helper: &syn::ItemFn,
    call_args: &[syn::Expr],
) -> syn::Expr {
    let last = b.stmts.pop().expect("RETURN wrapper missing");
    let syn::Stmt::Expr(
        syn::Expr::Call(syn::ExprCall {
            func: call_func,
            args: orig_args,
            ..
        }),
        None,
    ) = last
    else {
        panic!("RETURN tail is not a call expression");
    };
    let syn::Expr::Paren(syn::ExprParen { expr: iife, .. }) = &*call_func else {
        panic!("RETURN call func is not a parenthesized IIFE");
    };
    let syn::Expr::Block(syn::ExprBlock { block: iife_block, .. }) = &**iife else {
        panic!("RETURN IIFE is not a block");
    };
    assert_eq!(
        iife_block.stmts.len(),
        2,
        "RETURN IIFE must hold exactly the helper def and its path"
    );
    let syn::Stmt::Item(Item::Fn(def)) = &iife_block.stmts[0] else {
        panic!("RETURN IIFE first statement is not the helper fn");
    };
    assert_eq!(
        canonical(&[Item::Fn(def.clone())]),
        canonical(&[Item::Fn(helper.clone())]),
        "RETURN helper definition differs from plan"
    );
    // The second statement is the bare fn path (the call target).
    let syn::Stmt::Expr(syn::Expr::Path(syn::ExprPath { path, .. }), None) = &iife_block.stmts[1]
    else {
        panic!("RETURN IIFE second statement is not the bare helper path");
    };
    assert!(
        path.is_ident(&helper.sig.ident),
        "RETURN IIFE path does not name the planned helper"
    );
    // The call arguments: orig_args[0] is the original tail ($value);
    // orig_args[1..] must match the planned bindings in order (the plan's
    // $value entry was parsed as the identifier `value`, which we skip).
    let planned: Vec<&syn::Expr> = call_args
        .iter()
        .filter(|e| !matches!(e, syn::Expr::Path(p) if p.path.is_ident("value")))
        .collect();
    assert_eq!(
        orig_args.len(),
        planned.len() + 1,
        "RETURN wrapper arity mismatch"
    );
    for (idx, arg) in orig_args.iter().enumerate().skip(1) {
        assert_eq!(
            canonical_expr(arg),
            canonical_expr(planned[idx - 1]),
            "RETURN binding #{idx} differs from plan"
        );
    }
    orig_args.into_iter().next().unwrap()
}

fn canonical_expr(e: &syn::Expr) -> String {
    prettyplease::unparse(&syn::File {
        shebang: None,
        attrs: vec![],
        items: vec![syn::Item::Fn(syn::parse_quote! {
            fn audit_expr() { #e }
        })],
    })
}

fn main() {
    let args: Vec<_> = env::args().collect();
    let plan: serde_json::Value = serde_json::from_slice(&fs::read(&args[3]).unwrap()).unwrap();
    let mut trees = BTreeMap::new();
    for p in plan["injections"].as_array().unwrap() {
        let file = p["file"].as_str().unwrap();
        let tree = trees.entry(file).or_insert_with(|| {
            syn::parse_file(&fs::read_to_string(Path::new(&args[2]).join(file)).unwrap()).unwrap()
        });
        let node: Vec<usize> = p["node"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap() as usize)
            .collect();
        let kind = p["kind"].as_str().unwrap();
        assert_eq!(p["cancellable"], false);
        let helper: syn::ItemFn = syn::parse_str(p["helper_source"].as_str().unwrap()).unwrap();
        let name = &helper.sig.ident;
        let call_args: Vec<syn::Expr> = p["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| {
                let v = b["value"].as_str().unwrap();
                // "$value" is the RETURN-injection placeholder for the wrapped
                // tail expression; it is substituted with the original tail by
                // the stage and is not valid Rust on its own.
                if v == "$value" {
                    syn::parse_quote!(value)
                } else {
                    syn::parse_str::<syn::Expr>(v).unwrap()
                }
            })
            .collect();
        let b = block(&mut tree.items, &node);
        match kind {
            "HEAD" => {
                let expected: syn::Stmt =
                    syn::parse2(quote!({#helper #name(#(#call_args),*)};)).unwrap();
                let actual = b.stmts.first().expect("HEAD missing");
                let actual_fn: Item = syn::parse_quote!(fn audit() {#actual});
                let expected_fn: Item = syn::parse_quote!(fn audit() {#expected});
                assert_eq!(
                    canonical(&[actual_fn]),
                    canonical(&[expected_fn]),
                    "HEAD differs from planned helper/bindings"
                );
                b.stmts.remove(0);
            }
            "RETURN" => {
                // The stage wraps the original tail expression in an IIFE:
                //   ({ fn helper(..) {..} helper })(#orig_args)
                // where the "$value" binding receives the original tail.
                // Verify the wrapper matches the plan, unwrap it and restore
                // the original tail so the AST comparison below holds.
                let tail = pop_return_wrapper(b, &helper, &call_args);
                b.stmts.push(syn::Stmt::Expr(tail, None));
            }
            "OVERWRITE" => {
                let orig_file =
                    syn::parse_file(&fs::read_to_string(Path::new(&args[1]).join(file)).unwrap())
                        .unwrap();
                let mut orig_items = orig_file.items;
                let orig_b = block(&mut orig_items, &node);
                *b = orig_b.clone();
            }
            other => panic!("unexpected injection kind in audit: {other}"),
        }
    }
    for m in plan["modules"].as_array().unwrap() {
        let file = m["file"].as_str().unwrap();
        let tree = trees.entry(file).or_insert_with(|| {
            syn::parse_file(&fs::read_to_string(Path::new(&args[2]).join(file)).unwrap()).unwrap()
        });
        let index = tree
            .items
            .iter()
            .position(|i| matches!(i, Item::Mod(v) if v.ident == m["name"].as_str().unwrap()))
            .expect("support module missing");
        let Item::Mod(module) = tree.items.remove(index) else {
            unreachable!()
        };
        let payload = syn::parse_file(m["source"].as_str().unwrap()).unwrap();
        assert!(module.attrs.is_empty());
        let mut expected = module.clone();
        expected.content.as_mut().unwrap().1 = payload.items;
        assert!(
            canonical(&[Item::Mod(module)]) == canonical(&[Item::Mod(expected)]),
            "support module differs from planned source"
        );
    }
    for (file, tree) in trees {
        let original =
            syn::parse_file(&fs::read_to_string(Path::new(&args[1]).join(file)).unwrap()).unwrap();
        let original_normalized = syn::parse_file(&prettyplease::unparse(&original)).unwrap();
        assert!(
            prettyplease::unparse(&original_normalized) == prettyplease::unparse(&tree),
            "unexpected source change in {file}"
        );
    }
    println!(
        "all original ASTs preserved apart from the planned injections and support modules"
    );
}

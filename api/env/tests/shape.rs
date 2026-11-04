use kernel_api_env as env;

#[test]
fn environment_marks_have_no_domain_dependencies() {
    use std::{fs, path::Path};
    use syn::visit::{self, Visit};

    #[derive(Default)]
    struct References(Vec<String>);
    impl<'a> Visit<'a> for References {
        fn visit_path(&mut self, path: &'a syn::Path) {
            self.0.push(path.segments.iter().map(|part| part.ident.to_string()).collect::<Vec<_>>().join("::"));
            visit::visit_path(self, path);
        }
        fn visit_item_use(&mut self, item: &'a syn::ItemUse) {
            fn collect(tree: &syn::UseTree, prefix: &str, out: &mut Vec<String>) {
                match tree {
                    syn::UseTree::Path(path) => collect(&path.tree, &format!("{prefix}{}::", path.ident), out),
                    syn::UseTree::Group(group) => for tree in &group.items { collect(tree, prefix, out); },
                    syn::UseTree::Name(name) => out.push(format!("{prefix}{}", name.ident)),
                    syn::UseTree::Rename(name) => out.push(format!("{prefix}{}", name.ident)),
                    syn::UseTree::Glob(_) => out.push(prefix.trim_end_matches("::").to_owned()),
                }
            }
            collect(&item.tree, "", &mut self.0);
        }
    }

    let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/marks.rs")).unwrap();
    let mut paths = References::default();
    paths.visit_file(&syn::parse_file(&source).unwrap());
    assert!(!paths.0.iter().any(|path| {
        path.starts_with("crate::system")
            || path.starts_with("crate::driver")
            || path.starts_with("crate::service")
            || path.starts_with("protocol::")
    }));
}

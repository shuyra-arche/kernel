use alloc::{string::String, vec::Vec};

mod paragraph;
pub use paragraph::Paragraph;

#[derive(Default)]
pub struct Report {
    pub paras: Vec<Paragraph>,
}

impl Report {
    pub fn paragraph(&mut self, title: Option<String>) -> &mut Paragraph {
        self.paras.push(Paragraph {
            title,
            items: Vec::new(),
        });
        self.paras.last_mut().expect("just pushed")
    }
}

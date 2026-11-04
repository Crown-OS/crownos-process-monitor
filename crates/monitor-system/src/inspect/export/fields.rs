use std::borrow::Cow;

/// Fields an export usually holds; enough to avoid regrowing.
const TYPICAL_FIELD_COUNT: usize = 64;

pub(super) struct Field<'a> {
    pub section: &'static str,
    pub name: Cow<'a, str>,
    pub value: Cow<'a, str>,
}

/// Collects non-empty fields, borrowing from the details where it can.
pub(super) struct Fields<'a> {
    section: &'static str,
    list: Vec<Field<'a>>,
}

impl Default for Fields<'_> {
    fn default() -> Self {
        Self {
            section: "",
            list: Vec::with_capacity(TYPICAL_FIELD_COUNT),
        }
    }
}

impl<'a> Fields<'a> {
    pub fn section(&mut self, section: &'static str) -> &mut Self {
        self.section = section;
        self
    }

    pub fn add(
        &mut self,
        name: impl Into<Cow<'a, str>>,
        value: impl Into<Cow<'a, str>>,
    ) -> &mut Self {
        let value = value.into();
        if !value.is_empty() {
            self.list.push(Field {
                section: self.section,
                name: name.into(),
                value,
            });
        }
        self
    }

    pub fn add_some<Value: Into<Cow<'a, str>>>(
        &mut self,
        name: &'static str,
        value: Option<Value>,
    ) -> &mut Self {
        match value {
            Some(value) => self.add(name, value),
            None => self,
        }
    }

    pub fn into_list(self) -> Vec<Field<'a>> {
        self.list
    }
}

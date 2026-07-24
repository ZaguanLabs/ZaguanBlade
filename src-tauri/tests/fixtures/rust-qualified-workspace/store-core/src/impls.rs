impl crate::store::Store {
    pub fn new() -> Self {
        Self
    }
}

impl crate::Maker for crate::store::Store {
    fn make() -> Self {
        Self
    }
}

impl<T> crate::store::Generic<T> {
    pub fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl crate::store::r#Raw {
    pub fn r#build() -> Self {
        Self
    }
}

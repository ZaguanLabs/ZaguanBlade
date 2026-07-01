class Base:
    def base_method(self):
        return 1


class Derived(Base):
    def go(self):
        return self.base_method()

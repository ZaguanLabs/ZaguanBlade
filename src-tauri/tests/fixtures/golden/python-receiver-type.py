class A:
    def run(self):
        return 1

    def go(self):
        self.run()
        x = B()
        x.run()


class B:
    def run(self):
        return 2

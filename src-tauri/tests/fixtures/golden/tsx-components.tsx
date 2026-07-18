// Section: toolbar shell
// PascalCase JSX elements are call observations; native/lowercase tags are not.
function Header() {
  return (
    <nav>
      <LanguageSwitcher />
      <RegionSelector></RegionSelector>
      <ModeToggle checked={true} />
      <Dialog.Trigger />
      {/* Section: native controls */}
      <button>Menu</button>
      <motion.div />
      <span>Ready</span>
    </nav>
  );
}

// Section: settings body
function Settings() {
  return (
    <section>
      <LanguageSwitcher compact />
      <RegionSelector />
      <ModeToggle />
    </section>
  );
}

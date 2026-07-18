import { LanguageSwitcher } from "./language-switcher";
import { RegionSelector } from "./region-selector";
import { ModeToggle } from "./mode-toggle";

export function Header() {
  return (
    <nav>
      {/* Mobile navigation */}
      <LanguageSwitcher />
      <RegionSelector />
      <ModeToggle />
      <button>Menu</button>
      <motion.div />
      {/* Right side actions */}
      <LanguageSwitcher />
      <RegionSelector></RegionSelector>
      <ModeToggle />
    </nav>
  );
}

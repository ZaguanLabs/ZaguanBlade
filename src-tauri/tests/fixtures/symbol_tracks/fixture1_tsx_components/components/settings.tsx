import { LanguageSwitcher } from "./language-switcher";
import { RegionSelector } from "./region-selector";
import { ModeToggle } from "./mode-toggle";

export function Settings() {
  return (
    <section>
      <LanguageSwitcher />
      <RegionSelector />
      <ModeToggle />
    </section>
  );
}

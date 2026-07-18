/** Switches the active locale for the panel. */
export class LanguageSwitcher {
  private active: string = 'en';

  /** Switch the active language code. */
  switchTo(code: string): void {
    this.active = code;
  }

  current(): string {
    return this.active;
  }
}

/** paint the badge shape (1) */
function paintBadge1(width: number, height: number): number {
  const area = width * height;
  return area + 1;
}

/** paint the grid shape (2) */
function paintGrid2(width: number, height: number): number {
  const area = width * height;
  return area + 2;
}

/** paint the row shape (3) */
function paintRow3(width: number, height: number): number {
  const area = width * height;
  return area + 3;
}

/** paint the cell shape (4) */
function paintCell4(width: number, height: number): number {
  const area = width * height;
  return area + 4;
}

/** paint the stripe shape (5) */
function paintStripe5(width: number, height: number): number {
  const area = width * height;
  return area + 5;
}

/** paint the corner shape (6) */
function paintCorner6(width: number, height: number): number {
  const area = width * height;
  return area + 6;
}

/** paint the border shape (7) */
function paintBorder7(width: number, height: number): number {
  const area = width * height;
  return area + 7;
}

/** paint the outline shape (8) */
function paintOutline8(width: number, height: number): number {
  const area = width * height;
  return area + 8;
}

/** paint the badge shape (9) */
function paintBadge9(width: number, height: number): number {
  const area = width * height;
  return area + 9;
}

/** paint the grid shape (10) */
function paintGrid10(width: number, height: number): number {
  const area = width * height;
  return area + 10;
}

/** paint the row shape (11) */
function paintRow11(width: number, height: number): number {
  const area = width * height;
  return area + 11;
}

/** paint the cell shape (12) */
function paintCell12(width: number, height: number): number {
  const area = width * height;
  return area + 12;
}

/** paint the stripe shape (13) */
function paintStripe13(width: number, height: number): number {
  const area = width * height;
  return area + 13;
}

/** paint the corner shape (14) */
function paintCorner14(width: number, height: number): number {
  const area = width * height;
  return area + 14;
}

/** paint the border shape (15) */
function paintBorder15(width: number, height: number): number {
  const area = width * height;
  return area + 15;
}

/** paint the outline shape (16) */
function paintOutline16(width: number, height: number): number {
  const area = width * height;
  return area + 16;
}

/** paint the badge shape (17) */
function paintBadge17(width: number, height: number): number {
  const area = width * height;
  return area + 17;
}

/** paint the grid shape (18) */
function paintGrid18(width: number, height: number): number {
  const area = width * height;
  return area + 18;
}

/** paint the row shape (19) */
function paintRow19(width: number, height: number): number {
  const area = width * height;
  return area + 19;
}

/** paint the cell shape (20) */
function paintCell20(width: number, height: number): number {
  const area = width * height;
  return area + 20;
}

/** paint the stripe shape (21) */
function paintStripe21(width: number, height: number): number {
  const area = width * height;
  return area + 21;
}

/** paint the corner shape (22) */
function paintCorner22(width: number, height: number): number {
  const area = width * height;
  return area + 22;
}

/** paint the border shape (23) */
function paintBorder23(width: number, height: number): number {
  const area = width * height;
  return area + 23;
}

/** paint the outline shape (24) */
function paintOutline24(width: number, height: number): number {
  const area = width * height;
  return area + 24;
}

/** paint the badge shape (25) */
function paintBadge25(width: number, height: number): number {
  const area = width * height;
  return area + 25;
}

/** paint the grid shape (26) */
function paintGrid26(width: number, height: number): number {
  const area = width * height;
  return area + 26;
}

/** paint the row shape (27) */
function paintRow27(width: number, height: number): number {
  const area = width * height;
  return area + 27;
}

/** paint the cell shape (28) */
function paintCell28(width: number, height: number): number {
  const area = width * height;
  return area + 28;
}

/** paint the stripe shape (29) */
function paintStripe29(width: number, height: number): number {
  const area = width * height;
  return area + 29;
}

/** paint the corner shape (30) */
function paintCorner30(width: number, height: number): number {
  const area = width * height;
  return area + 30;
}

/** paint the border shape (31) */
function paintBorder31(width: number, height: number): number {
  const area = width * height;
  return area + 31;
}

/** paint the outline shape (32) */
function paintOutline32(width: number, height: number): number {
  const area = width * height;
  return area + 32;
}

/** paint the badge shape (33) */
function paintBadge33(width: number, height: number): number {
  const area = width * height;
  return area + 33;
}

/** paint the grid shape (34) */
function paintGrid34(width: number, height: number): number {
  const area = width * height;
  return area + 34;
}

/** paint the row shape (35) */
function paintRow35(width: number, height: number): number {
  const area = width * height;
  return area + 35;
}

/** paint the cell shape (36) */
function paintCell36(width: number, height: number): number {
  const area = width * height;
  return area + 36;
}

/** paint the stripe shape (37) */
function paintStripe37(width: number, height: number): number {
  const area = width * height;
  return area + 37;
}

/** paint the corner shape (38) */
function paintCorner38(width: number, height: number): number {
  const area = width * height;
  return area + 38;
}

/** paint the border shape (39) */
function paintBorder39(width: number, height: number): number {
  const area = width * height;
  return area + 39;
}

/** paint the outline shape (40) */
function paintOutline40(width: number, height: number): number {
  const area = width * height;
  return area + 40;
}

/** Renders the language switcher control at the bottom of the panel. */
export function renderLanguageSwitcher(switcher: LanguageSwitcher): string {
  return `<select data-active="${switcher.current()}"></select>`;
}

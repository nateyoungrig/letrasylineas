// The colour scheme, kept per device in localStorage. Palettes themselves live in app.css.
export const PALETTES = [
  { id: "rosa", name: "Rosa mexicano", blurb: "Hot pink with turquoise" },
  { id: "talavera", name: "Talavera", blurb: "Cobalt tile blue with ochre" },
  { id: "cempasuchil", name: "Cempasúchil", blurb: "Marigold on Día de Muertos violet" },
  { id: "jade", name: "Jade", blurb: "Oaxacan jade on obsidian, cochineal red" },
] as const;

export type PaletteId = (typeof PALETTES)[number]["id"];

const KEY = "palette";
const DEFAULT: PaletteId = "rosa";

export function savedPalette(): PaletteId {
  try {
    const id = localStorage.getItem(KEY);
    if (PALETTES.some((p) => p.id === id)) return id as PaletteId;
  } catch {}
  return DEFAULT;
}

export function applyPalette(id: PaletteId) {
  document.documentElement.dataset.palette = id;
  try {
    localStorage.setItem(KEY, id);
  } catch {}
}

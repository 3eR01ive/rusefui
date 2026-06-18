/** Типы с реализацией логики в Rust (`rusefui-runtime`). Должны совпадать с LogicComponentType. */
const RUST_LOGIC_TYPES = new Set([
  "connection",
  "simulation",
  "dyno",
  "knock",
  "knock-threshold",
  "knock-spectrum",
  "config-table",
  "ignition-table",
  "command",
  "lua-script",
  "ini-command-button",
]);

export function requiresRustLogic(componentType: string): boolean {
  return RUST_LOGIC_TYPES.has(componentType);
}

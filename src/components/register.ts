import { registerComponent } from "../core/registry";
import StackLayout from "./builtins/StackLayout.vue";
import RowLayout from "./builtins/RowLayout.vue";
import SectionLayout from "./builtins/SectionLayout.vue";
import CompositeLayout from "./builtins/CompositeLayout.vue";
import TextBlock from "./builtins/TextBlock.vue";
import ConnectionPanel from "./builtins/ConnectionPanel.vue";
import SimulationPanel from "./builtins/SimulationPanel.vue";
import ScalarField from "./builtins/ScalarField.vue";
import OutputValue from "./builtins/OutputValue.vue";

let registered = false;

/**
 * Регистрация всех типов компонентов, реализованных в коде.
 * Только зарегистрированные типы можно использовать в YAML (`type: …`).
 */
export function registerBuiltinComponents(): void {
  if (registered) return;
  registered = true;

  registerComponent(
    {
      type: "stack",
      label: "Stack",
      mode: "display",
      isContainer: true,
      description: "Вертикальная группа",
    },
    StackLayout,
  );

  registerComponent(
    {
      type: "row",
      label: "Row",
      mode: "display",
      isContainer: true,
      description: "Горизонтальная группа",
    },
    RowLayout,
  );

  registerComponent(
    {
      type: "section",
      label: "Section",
      mode: "display",
      isContainer: true,
      description: "Секция с заголовком",
    },
    SectionLayout,
  );

  registerComponent(
    {
      type: "composite",
      label: "Composite",
      mode: "display",
      isContainer: true,
      description: "Корень составного компонента из YAML-файла",
    },
    CompositeLayout,
  );

  registerComponent(
    {
      type: "text",
      label: "Text",
      mode: "display",
      isContainer: false,
    },
    TextBlock,
  );

  registerComponent(
    {
      type: "connection",
      label: "Connection",
      mode: "display",
      isContainer: false,
      description: "Подключение к ECU по serial",
    },
    ConnectionPanel,
  );

  registerComponent(
    {
      type: "simulation",
      label: "Simulation",
      mode: "display",
      isContainer: false,
      description: "ECU trigger stimulator (RPM + cmd Z)",
    },
    SimulationPanel,
  );

  registerComponent(
    {
      type: "scalar-field",
      label: "Scalar field",
      mode: "edit",
      isContainer: false,
      description: "Поле калибровки (config page)",
    },
    ScalarField,
  );

  registerComponent(
    {
      type: "output-value",
      label: "Output value",
      mode: "display",
      isContainer: false,
      description: "Значение из outputChannels",
    },
    OutputValue,
  );
}

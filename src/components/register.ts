import type { Component } from "vue";
import { registerComponent } from "../core/registry";
import StackLayout from "./builtins/StackLayout.vue";
import RowLayout from "./builtins/RowLayout.vue";
import SectionLayout from "./builtins/SectionLayout.vue";
import CompositeLayout from "./builtins/CompositeLayout.vue";
import TextBlock from "./builtins/TextBlock.vue";
import ConnectionPanel from "./builtins/ConnectionPanel.vue";
import SimulationPanel from "./builtins/SimulationPanel.vue";
import ScalarField from "./builtins/ScalarField.vue";
import StringField from "./builtins/StringField.vue";
import EnumField from "./builtins/EnumField.vue";
import ConfigTable from "./builtins/ConfigTable.vue";
import ConfigCurve from "./builtins/ConfigCurve.vue";
import IniPanelsBrowser from "./builtins/IniPanelsBrowser.vue";
import OutputChart from "./builtins/OutputChart.vue";
import CompositeChart from "./builtins/CompositeChart.vue";
import OutputValue from "./builtins/OutputValue.vue";
import Dyno from "./builtins/Dyno.vue";
import Spectrogram from "./builtins/Spectrogram.vue";
import ConfigChecklist from "./builtins/ConfigChecklist.vue";
import ProjectTimeline from "./builtins/ProjectTimeline.vue";

let registered = false;

/** type → Vue SFC. Nav/container — в YAML инстанса panel. */
function reg(type: string, component: Component): void {
  registerComponent({ type, label: type, mode: "display", isContainer: false }, component);
}

export function registerBuiltinComponents(): void {
  if (registered) return;
  registered = true;

  reg("stack", StackLayout);
  reg("row", RowLayout);
  reg("section", SectionLayout);
  reg("composite", CompositeLayout);
  reg("text", TextBlock);
  reg("connection", ConnectionPanel);
  reg("simulation", SimulationPanel);
  reg("scalar-field", ScalarField);
  reg("string-field", StringField);
  reg("enum-field", EnumField);
  reg("config-table", ConfigTable);
  reg("curve", ConfigCurve);
  reg("ini-panels-browser", IniPanelsBrowser);
  reg("output-chart", OutputChart);
  reg("composite-chart", CompositeChart);
  reg("output-value", OutputValue);
  reg("dyno", Dyno);
  reg("spectrogram", Spectrogram);
  reg("config-checklist", ConfigChecklist);
  reg("project-timeline", ProjectTimeline);
}

import { createApp } from "vue";
import App from "./App.vue";
import "./styles.css";
import { registerBuiltinComponents } from "./components/register";

registerBuiltinComponents();
createApp(App).mount("#app");

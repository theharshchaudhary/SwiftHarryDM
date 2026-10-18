import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";
import { windowParams } from "./api";
import Main from "./windows/Main";
import AddUrl from "./windows/AddUrl";
import FileInfo from "./windows/FileInfo";
import Progress from "./windows/Progress";
import Complete from "./windows/Complete";
import Options from "./windows/Options";
import Scheduler from "./windows/Scheduler";
import Batch from "./windows/Batch";
import Links from "./windows/Links";
import Properties from "./windows/Properties";
import { About, Countdown, Unknown } from "./windows/Small";

// Every window loads this page; ?w= selects what it shows.
const WINDOWS: Record<string, () => JSX.Element | null> = {
  main: Main,
  add: AddUrl,
  info: FileInfo,
  progress: Progress,
  complete: Complete,
  options: Options,
  scheduler: Scheduler,
  batch: Batch,
  links: Links,
  properties: Properties,
  about: About,
  countdown: Countdown,
};

const View = WINDOWS[windowParams().get("w") ?? "main"] ?? Unknown;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <View />
  </React.StrictMode>,
);

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
import Pairing from "./windows/Pairing";
import Browser from "./windows/Browser";
import Drop from "./windows/Drop";
import Login from "./windows/Login";
import Tips from "./windows/Tips";
import Grabber from "./windows/Grabber";

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
  pairing: Pairing,
  browser: Browser,
  drop: Drop,
  login: Login,
  tips: Tips,
  grabber: Grabber,
};

const TITLES: Record<string, string> = {
  main: "SwiftHarryDM",
  add: "Add download",
  info: "Download file info",
  progress: "Download progress",
  complete: "Download complete",
  options: "Options",
  scheduler: "Scheduler",
  batch: "Batch download",
  links: "Choose files to download",
  properties: "Properties",
  about: "About SwiftHarryDM",
  countdown: "SwiftHarryDM",
  pairing: "Connect browser extension",
  browser: "Connect your browser",
  drop: "Drop target",
  login: "Login required",
  tips: "Tip of the day",
  grabber: "Site grabber",
};

const kind = windowParams().get("w") ?? "main";
document.title = TITLES[kind] ?? "SwiftHarryDM";
const View = WINDOWS[kind] ?? Unknown;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <View />
  </React.StrictMode>,
);

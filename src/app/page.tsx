"use client";

import StatusBar from "../components/StatusBar";
import TerminalComponent from "../components/Terminal";

export default function Home() {
  return (
    <div
      style={{
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        backgroundColor: "#1e1e1e",
        overflow: "hidden",
      }}
    >
      <StatusBar />
      <TerminalComponent />
    </div>
  );
}

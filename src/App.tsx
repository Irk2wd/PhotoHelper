import { useState } from "react";
import NavSidebar from "./components/NavSidebar";
import SyncView from "./features/sync/SyncView";
import ClassifyView from "./features/classify/ClassifyView";
import ProcessView from "./features/process/ProcessView";
import "./App.css";

type ViewId = "sync" | "classify" | "process";

function App() {
  const [currentView, setCurrentView] = useState<ViewId>("sync");

  return (
    <div className="app-layout">
      <NavSidebar
        currentView={currentView}
        onNavigate={(v) => setCurrentView(v as ViewId)}
      />
      <main className="app-content">
        {currentView === "sync" && <SyncView />}
        {currentView === "classify" && <ClassifyView />}
        {currentView === "process" && <ProcessView />}
      </main>
    </div>
  );
}

export default App;

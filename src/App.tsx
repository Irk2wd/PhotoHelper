import { useState } from "react";
import NavSidebar from "./components/NavSidebar";
import SyncView from "./features/sync/SyncView";
import ClassifyView from "./features/classify/ClassifyView";
import ProcessView from "./features/process/ProcessView";
import TransferView from "./features/transfer/TransferView";
import "./App.css";

type ViewId = "sync" | "classify" | "process" | "transfer";

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
        {currentView === "transfer" && <TransferView />}
      </main>
    </div>
  );
}

export default App;

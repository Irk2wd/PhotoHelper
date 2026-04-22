import { useState } from "react";
import NavSidebar from "./components/NavSidebar";
import SyncView from "./features/sync/SyncView";
import "./App.css";

type ViewId = "sync";

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
      </main>
    </div>
  );
}

export default App;

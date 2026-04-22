import type { ReactElement } from "react";
import "./NavSidebar.css";

interface NavItem {
  id: string;
  label: string;
  icon: ReactElement;
}

const NAV_ITEMS: NavItem[] = [
  {
    id: "sync",
    label: "HEIF/RAW 同步",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M21 2v6h-6" />
        <path d="M3 12a9 9 0 0 1 15-6.7L21 8" />
        <path d="M3 22v-6h6" />
        <path d="M21 12a9 9 0 0 1-15 6.7L3 16" />
      </svg>
    ),
  },
];

interface Props {
  currentView: string;
  onNavigate: (id: string) => void;
}

export default function NavSidebar({ currentView, onNavigate }: Props) {
  return (
    <nav className="nav-sidebar">
      {/* Logo */}
      <div className="nav-logo">
        <div className="nav-logo-icon">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
            <circle cx="8.5" cy="8.5" r="1.5" />
            <polyline points="21 15 16 10 5 21" />
          </svg>
        </div>
        <span className="nav-logo-text">PhotoHelper</span>
      </div>

      {/* 分隔线 */}
      <div className="nav-divider" />

      {/* 导航项 */}
      <ul className="nav-list">
        {NAV_ITEMS.map((item) => (
          <li key={item.id}>
            <button
              className={`nav-item ${currentView === item.id ? "nav-item--active" : ""}`}
              onClick={() => onNavigate(item.id)}
            >
              <span className="nav-item-icon">{item.icon}</span>
              <span className="nav-item-label">{item.label}</span>
            </button>
          </li>
        ))}
      </ul>

      {/* 底部版本 */}
      <div className="nav-footer">
        <span className="nav-version">v0.1.0</span>
      </div>
    </nav>
  );
}

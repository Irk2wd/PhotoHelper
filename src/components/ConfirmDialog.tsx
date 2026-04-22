import { useState, useEffect } from "react";
import "./ConfirmDialog.css";

interface Props {
  open: boolean;
  toDelete: string[];
  onCancel: () => void;
  onConfirm: () => void;
}

export default function ConfirmDialog({ open, toDelete, onCancel, onConfirm }: Props) {
  const [checked, setChecked] = useState(false);

  // 每次打开时重置勾选
  useEffect(() => {
    if (open) setChecked(false);
  }, [open]);

  if (!open) return null;

  const isEmpty = toDelete.length === 0;

  return (
    <div className="cd-overlay" onClick={onCancel}>
      <div className="cd-panel" onClick={(e) => e.stopPropagation()}>
        {/* 标题 */}
        <div className="cd-header">
          <div className="cd-title-row">
            <span className="cd-icon">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="3 6 5 6 21 6" />
                <path d="M19 6l-1 14H6L5 6" />
                <path d="M10 11v6" />
                <path d="M14 11v6" />
                <path d="M9 6V4h6v2" />
              </svg>
            </span>
            <h2 className="cd-title">确认同步操作</h2>
          </div>
          <button className="cd-close" onClick={onCancel} aria-label="关闭">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        {isEmpty ? (
          <div className="cd-empty">
            <p>当前同步方向下没有需要删除的文件。</p>
          </div>
        ) : (
          <>
            {/* 摘要 */}
            <div className="cd-summary">
              将把以下 <strong>{toDelete.length}</strong> 个文件移入系统回收站（可恢复）：
            </div>

            {/* 文件列表 */}
            <div className="cd-file-list">
              {toDelete.map((p) => (
                <div key={p} className="cd-file-item">
                  <span className="cd-file-icon">
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                      <polyline points="14 2 14 8 20 8" />
                    </svg>
                  </span>
                  <span className="cd-file-path" title={p}>{p}</span>
                </div>
              ))}
            </div>

            {/* 确认勾选 */}
            <label className="cd-checkbox-label">
              <input
                type="checkbox"
                className="cd-checkbox"
                checked={checked}
                onChange={(e) => setChecked(e.target.checked)}
              />
              <span>我已确认，以上文件将被移入回收站</span>
            </label>
          </>
        )}

        {/* 操作按钮 */}
        <div className="cd-footer">
          <button className="cd-btn cd-btn--cancel" onClick={onCancel}>
            取消
          </button>
          {!isEmpty && (
            <button
              className="cd-btn cd-btn--confirm"
              onClick={onConfirm}
              disabled={!checked}
            >
              确认执行
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

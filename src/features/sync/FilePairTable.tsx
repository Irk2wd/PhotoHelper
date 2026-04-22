import { PhotoPair, PairStatus } from "./types";
import "./FilePairTable.css";

interface Props {
  pairs: PhotoPair[];
}

const STATUS_LABEL: Record<PairStatus, string> = {
  paired: "成对",
  heif_only: "仅 HEIF",
  raw_only: "仅 RAW",
};

function fileBasename(path: string): string {
  return path.replace(/\\/g, "/").split("/").pop() ?? path;
}

export default function FilePairTable({ pairs }: Props) {
  if (pairs.length === 0) return null;

  return (
    <div className="fpt-wrapper">
      <table className="fpt-table">
        <thead>
          <tr>
            <th className="fpt-th fpt-col-stem">文件名</th>
            <th className="fpt-th fpt-col-file">HEIF</th>
            <th className="fpt-th fpt-col-file">RAW</th>
            <th className="fpt-th fpt-col-status">状态</th>
          </tr>
        </thead>
        <tbody>
          {pairs.map((p) => (
            <tr key={p.stem} className={`fpt-row fpt-row--${p.status}`}>
              <td className="fpt-td fpt-col-stem">
                <span className="fpt-stem">{p.stem}</span>
              </td>
              <td className="fpt-td fpt-col-file">
                {p.heifPath ? (
                  <span className="fpt-file fpt-file--present" title={p.heifPath}>
                    <span className="fpt-check">✓</span>
                    {fileBasename(p.heifPath)}
                  </span>
                ) : (
                  <span className="fpt-file fpt-file--absent">—</span>
                )}
              </td>
              <td className="fpt-td fpt-col-file">
                {p.rawPath ? (
                  <span className="fpt-file fpt-file--present" title={p.rawPath}>
                    <span className="fpt-check">✓</span>
                    {fileBasename(p.rawPath)}
                  </span>
                ) : (
                  <span className="fpt-file fpt-file--absent">—</span>
                )}
              </td>
              <td className="fpt-td fpt-col-status">
                <span className={`fpt-badge fpt-badge--${p.status}`}>
                  {STATUS_LABEL[p.status]}
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

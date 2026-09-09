// SPDX-License-Identifier: AGPL-3.0-only

export function Logo() {
  return (
    <div className="header-brand">
      <div className="brand-logo" aria-hidden="true">
        <svg width="26" height="26" viewBox="0 0 24 24" fill="none">
          <path d="M12 2 2 7l10 5 10-5-10-5Z" stroke="#54d8ff" strokeWidth="1.8" />
          <path d="m2 12 10 5 10-5M2 17l10 5 10-5" stroke="#7c8cff" strokeWidth="1.8" />
        </svg>
      </div>
      <div className="brand-title">
        <span className="title-primary">TEMNION</span>
        <span className="title-sub">STUDIO</span>
        <span className="version-badge">v0.1</span>
      </div>
    </div>
  );
}

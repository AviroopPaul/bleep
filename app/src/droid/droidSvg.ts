// Ported from landing/index.html (DROID_FULL, DROID_HEAD). Original droid design.
export const DROID_FULL = `
<svg viewBox="0 0 120 150" aria-hidden="true">
  <ellipse class="shadow" cx="60" cy="142" rx="28" ry="5" fill="#1B1F24"/>
  <g class="bob">
    <ellipse class="flame" cx="60" cy="120" rx="9" ry="12" fill="#8FC1F2" opacity=".9" style="filter:blur(2.5px)"/>
    <path d="M50 110 h20 l-4 8 h-12z" fill="#9AA3AD" stroke="#1B1F24" stroke-width="2.4" stroke-linejoin="round"/>
    <rect x="17" y="76" width="13" height="24" rx="6.5" fill="#F7F6F2" stroke="#1B1F24" stroke-width="2.4"/>
    <rect x="90" y="76" width="13" height="24" rx="6.5" fill="#F7F6F2" stroke="#1B1F24" stroke-width="2.4"/>
    <rect x="20" y="82" width="7" height="3" rx="1.5" fill="#2F5C93"/><rect x="93" y="82" width="7" height="3" rx="1.5" fill="#2F5C93"/>
    <rect x="28" y="62" width="64" height="50" rx="20" fill="#F7F6F2" stroke="#1B1F24" stroke-width="2.6"/>
    <rect x="41" y="72" width="38" height="16" rx="5" fill="#2F5C93" stroke="#1B1F24" stroke-width="2"/>
    <rect x="45" y="76" width="9" height="3" rx="1.5" fill="#DCE6F1"/><rect x="57" y="76" width="18" height="3" rx="1.5" fill="#4C7DB8"/>
    <rect x="45" y="81" width="18" height="3" rx="1.5" fill="#4C7DB8"/><circle cx="72" cy="82.5" r="2.3" fill="#E9C46A"/>
    <path d="M44 96 h12 M44 101 h12" stroke="#9AA3AD" stroke-width="2.2" stroke-linecap="round"/>
    <circle cx="74" cy="99" r="4.5" fill="#B5532F" stroke="#1B1F24" stroke-width="2"/>
    <rect x="25" y="55" width="70" height="9" rx="4.5" fill="#C9CED4" stroke="#1B1F24" stroke-width="2.4"/>
    <line x1="60" y1="20" x2="60" y2="6" stroke="#1B1F24" stroke-width="2.4" stroke-linecap="round"/>
    <circle class="ant" cx="60" cy="6" r="4.6" stroke="#1B1F24" stroke-width="2"/>
    <path d="M22 56 A38 37 0 0 1 98 56 Z" fill="#F7F6F2" stroke="#1B1F24" stroke-width="2.6" stroke-linejoin="round"/>
    <path d="M27 47 A38 37 0 0 1 93 47" fill="none" stroke="#2F5C93" stroke-width="4.5" stroke-linecap="round" opacity=".9"/>
    <rect x="31" y="28" width="9" height="7" rx="2" fill="#2F5C93" stroke="#1B1F24" stroke-width="1.6" transform="rotate(-28 35 31)"/>
    <rect x="80" y="28" width="9" height="7" rx="2" fill="#2F5C93" stroke="#1B1F24" stroke-width="1.6" transform="rotate(28 85 31)"/>
    <g class="look">
      <circle cx="60" cy="38" r="14.5" fill="#1B1F24"/>
      <circle class="scan" cx="60" cy="38" r="18" fill="none" stroke="#4C7DB8" stroke-width="2" stroke-dasharray="6 6"/>
      <g class="lens">
        <circle cx="60" cy="38" r="9.5" fill="#2F5C93"/>
        <circle cx="60" cy="38" r="5" fill="#8FC1F2"/>
        <circle cx="56" cy="34" r="2.6" fill="#fff"/>
      </g>
    </g>
  </g>
</svg>`;
export const DROID_HEAD = `
<svg viewBox="22 0 76 64" aria-hidden="true" style="width:100%;height:auto;display:block">
  <line x1="60" y1="20" x2="60" y2="8" stroke="#1B1F24" stroke-width="3.5" stroke-linecap="round"/>
  <circle cx="60" cy="7" r="5.5" fill="#E9C46A" stroke="#1B1F24" stroke-width="2.6"/>
  <path d="M22 58 A38 37 0 0 1 98 58 Z" fill="#F7F6F2" stroke="#1B1F24" stroke-width="3.5" stroke-linejoin="round"/>
  <path d="M27 49 A38 37 0 0 1 93 49" fill="none" stroke="#2F5C93" stroke-width="5.5" stroke-linecap="round"/>
  <circle cx="60" cy="40" r="15" fill="#1B1F24"/><circle cx="60" cy="40" r="9.5" fill="#2F5C93"/><circle cx="56" cy="36" r="3" fill="#fff"/>
</svg>`;

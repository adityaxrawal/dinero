/** Minimal SVG logo mark used in the rail */
export default function LogoMark() {
  return (
    <svg
      width="20"
      height="20"
      viewBox="0 0 512 512"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <rect x="72" y="82" rx="22" ry="22" width="368" height="110" fill="#F8E7C9" />
      <rect x="72" y="214" rx="22" ry="22" width="368" height="216" fill="#F8E7C9" />
      <rect x="110" y="112" width="146" height="22" rx="6" fill="#064E3B" />
      <rect x="274" y="112" width="88" height="22" rx="6" fill="rgba(6,78,59,0.5)" />
      <path
        d="M132 355 L192 295 L252 340 L336 256"
        fill="none"
        stroke="#064E3B"
        strokeWidth="16"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

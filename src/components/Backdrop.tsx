/**
 * The colour fields behind every pane, and the SVG filter the balance lens
 * uses to bend them like water.
 */
export function Backdrop() {
  return (
    <div className="backdrop" aria-hidden="true">
      <div className="field f1" />
      <div className="field f2" />
      <div className="field f3" />
      <svg width="0" height="0" style={{ position: "absolute" }}>
        <filter
          id="tuken-refract"
          x="0"
          y="0"
          width="100%"
          height="100%"
          colorInterpolationFilters="sRGB"
        >
          <feTurbulence
            type="fractalNoise"
            baseFrequency="0.005 0.009"
            numOctaves="2"
            seed="11"
            result="noise"
          />
          <feGaussianBlur in="noise" stdDeviation="3" result="soft" />
          <feDisplacementMap
            in="SourceGraphic"
            in2="soft"
            scale="54"
            xChannelSelector="R"
            yChannelSelector="G"
          />
        </filter>
      </svg>
    </div>
  );
}

/** Home while its first answers are on the way (spec 6.3 "loading"): the
 *  reference's skeleton shapes, the 210px hero with four bars (lines 502-504)
 *  and the lower grid of one list panel and three stat blocks (line 750).
 *
 *  The shapes stand where the real cards will land, so nothing jumps when the
 *  data arrives. They are one busy region to assistive tech, not five empty
 *  boxes read out one by one. Nothing moves: the reference's shimmer layer
 *  (`.sk::after`) sits parked off to the left, and idle means still. */
export function HomeSkeleton() {
  return (
    <section className="scroll" aria-busy="true" aria-label="Loading your dictations">
      <div className="sk sk-hero" aria-hidden="true">
        <div className="skl" style={{ width: 180 }} />
        <div className="skl" style={{ marginTop: 22, height: 16, width: "92%" }} />
        <div className="skl" style={{ marginTop: 12, height: 16, width: "80%" }} />
        <div className="skl" style={{ marginTop: 12, height: 16, width: "60%" }} />
      </div>
      <div className="lower" aria-hidden="true">
        <div className="sk" />
        <div className="sk-col">
          <div className="sk" style={{ height: 120 }} />
          <div className="sk" style={{ height: 110 }} />
          <div className="sk" style={{ flex: 1 }} />
        </div>
      </div>
    </section>
  );
}

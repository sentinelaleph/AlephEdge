/** Fixed-height placeholder rows while a table's first load is in flight. */
export function SkeletonRows({ rows }: { rows: number }) {
  return (
    <div className="ae-skelrows" aria-busy="true">
      {Array.from({ length: rows }, (_, i) => (
        <span key={i} className="ae-skelrows__row" />
      ))}
    </div>
  );
}

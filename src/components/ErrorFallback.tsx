import type { FallbackProps } from "react-error-boundary";

function ErrorFallback({ error }: FallbackProps) {
  return (
    <div>
      <h2>An unexpected error has occurred.</h2>
      <pre>{error.message}</pre>
    </div>
  );
}
export default ErrorFallback;

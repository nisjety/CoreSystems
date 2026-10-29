const serviceNavigationKeys = new Set([
  "ArrowRight",
  "ArrowLeft",
  "Home",
  "End",
]);

export function getNextServiceIndex(
  key: string,
  currentIndex: number,
  serviceCount: number,
) {
  if (serviceCount === 0 || !serviceNavigationKeys.has(key)) {
    return null;
  }

  switch (key) {
    case "ArrowRight":
      return (currentIndex + 1) % serviceCount;
    case "ArrowLeft":
      return (currentIndex - 1 + serviceCount) % serviceCount;
    case "Home":
      return 0;
    case "End":
      return serviceCount - 1;
    default:
      return null;
  }
}

const productNavigationKeys = new Set([
  "ArrowRight",
  "ArrowLeft",
  "Home",
  "End",
]);

export function getNextOwnedProductIndex(
  key: string,
  currentIndex: number,
  productCount: number,
) {
  if (productCount === 0 || !productNavigationKeys.has(key)) {
    return null;
  }

  switch (key) {
    case "ArrowRight":
      return (currentIndex + 1) % productCount;
    case "ArrowLeft":
      return (currentIndex - 1 + productCount) % productCount;
    case "Home":
      return 0;
    case "End":
      return productCount - 1;
    default:
      return null;
  }
}

export type ServiceCardPointer = {
  x: number;
  y: number;
};

type ServiceCardMotionProfile = {
  horizontalTravel: number;
  rotation: number;
  scale: number;
  verticalTravel: number;
};

const serviceCardMotionProfiles: ServiceCardMotionProfile[] = [
  { horizontalTravel: 16, verticalTravel: 10, rotation: 0.7, scale: 1.004 },
  { horizontalTravel: -12, verticalTravel: 12, rotation: -0.6, scale: 1.035 },
  { horizontalTravel: 15, verticalTravel: -14, rotation: 0.8, scale: 1.065 },
  { horizontalTravel: -14, verticalTravel: -11, rotation: -0.72, scale: 1.09 },
  { horizontalTravel: 19, verticalTravel: 16, rotation: 0.86, scale: 1.12 },
];

export function getServiceCardMotionProfile(index: number) {
  return serviceCardMotionProfiles[index % serviceCardMotionProfiles.length];
}

export function getServiceCardPointerTransform(
  index: number,
  pointer: ServiceCardPointer,
  shouldReduceMotion: boolean,
) {
  if (shouldReduceMotion) {
    return { x: 0, y: 0, rotate: 0, scale: 1 };
  }

  const profile = getServiceCardMotionProfile(index);

  return {
    x: pointer.x * profile.horizontalTravel,
    y: pointer.y * profile.verticalTravel,
    rotate: pointer.x * profile.rotation,
    scale: profile.scale,
  };
}

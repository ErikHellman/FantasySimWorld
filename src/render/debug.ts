import * as THREE from 'three/webgpu';
import { CELL, navGrid } from '../world/navgrid';
import { places } from '../world/places';

/** Debug overlays (toggle with `?debug=nav` in the URL): blocked nav cells, slots and doors. */
export function buildDebugOverlay(): THREE.Group {
  const group = new THREE.Group();
  group.name = 'debug';

  let blocked = 0;
  for (let i = 0; i < navGrid.cost.length; i++) if ((navGrid.cost[i] as number) <= 0) blocked++;
  const cells = new THREE.InstancedMesh(
    new THREE.PlaneGeometry(CELL * 0.9, CELL * 0.9).rotateX(-Math.PI / 2),
    new THREE.MeshBasicMaterial({ color: 0xff3030, transparent: true, opacity: 0.35, depthWrite: false }),
    blocked,
  );
  const matrix = new THREE.Matrix4();
  let n = 0;
  for (let iz = 0; iz < navGrid.height; iz++) {
    for (let ix = 0; ix < navGrid.width; ix++) {
      if ((navGrid.cost[iz * navGrid.width + ix] as number) > 0) continue;
      matrix.makeTranslation(navGrid.originX + (ix + 0.5) * CELL, 0.12, navGrid.originZ + (iz + 0.5) * CELL);
      cells.setMatrixAt(n++, matrix);
    }
  }
  cells.frustumCulled = false;
  group.add(cells);

  const slotGeometry = new THREE.ConeGeometry(0.3, 0.9, 8).rotateX(Math.PI / 2);
  const slotMaterial = new THREE.MeshBasicMaterial({ color: 0x30a0ff });
  const doorMaterial = new THREE.MeshBasicMaterial({ color: 0xffe040 });
  const spawnMaterial = new THREE.MeshBasicMaterial({ color: 0xc040ff });
  const marker = (x: number, z: number, facing: number, material: THREE.Material): void => {
    const mesh = new THREE.Mesh(slotGeometry, material);
    mesh.position.set(x, 0.5, z);
    mesh.rotation.y = facing;
    group.add(mesh);
  };
  for (const place of places) {
    marker(place.door.x, place.door.z, place.facing, doorMaterial);
    for (const slot of place.slots) marker(slot.pos.x, slot.pos.z, slot.facing, slotMaterial);
    for (const point of place.spawnPoints) marker(point.x, point.z, 0, spawnMaterial);
    for (const plot of place.gravePlots) marker(plot.pos.x, plot.pos.z, plot.facing, spawnMaterial);
  }
  return group;
}

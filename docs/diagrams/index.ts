import { architecture } from './architecture';
import { flows } from './flows';
import { mediaWorkflows } from './media-workflows';
import { taskAndStorageWorkflows } from './task-and-storage-workflows';

export { escape, renderFigure } from './drawing';
export const diagrams = new Map([...architecture, ...flows, ...mediaWorkflows, ...taskAndStorageWorkflows].map(fig => [fig.id, fig]));

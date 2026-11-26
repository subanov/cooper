-- Tasks can be linked to a Contract (FR-TSK-004, FR-CON-006).
ALTER TABLE task_links DROP CONSTRAINT task_links_kind_check;
ALTER TABLE task_links ADD CONSTRAINT task_links_kind_check CHECK (kind IN ('ticket', 'change', 'contract'));

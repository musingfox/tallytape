-- Keep receipts.updated_at in sync with row updates.
-- WHEN guard avoids recursive trigger firing when the trigger itself updates the column.
CREATE TRIGGER trg_receipts_updated_at
AFTER UPDATE ON receipts
FOR EACH ROW
WHEN NEW.updated_at = OLD.updated_at
BEGIN
    UPDATE receipts SET updated_at = unixepoch() WHERE id = NEW.id;
END;

use super::CalibrationStagedRunObserverEvent;

pub(super) fn emit_observer_event(
    observer: &mut Option<&mut dyn FnMut(CalibrationStagedRunObserverEvent)>,
    event: CalibrationStagedRunObserverEvent,
) {
    if let Some(observer) = observer.as_deref_mut() {
        observer(event);
    }
}

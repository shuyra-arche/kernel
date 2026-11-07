use super::queue::Queue;
pub(super) enum Pending {
    Queue(Queue),
    Dead,
}

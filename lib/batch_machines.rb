# frozen_string_literal: true

require_relative 'batch_machines/version'

module BatchMachines
  # The core's `Admission::Flush(FlushReason)`: the item was counted and a
  # flush is due. `reason` is :size or :deadline. The other honoured push
  # answer, `Admission::Buffered`, is the symbol :buffered. A Flush holds
  # only a `FlushReason`: any other symbol raises RangeError, a non-Symbol
  # TypeError, whether the gem built it or you did.
  Flush = Data.define(:reason) do
    def initialize(reason:)
      reason = BatchMachines.__send__(:__flush, reason)
      super
    end
  end
end

# Native is required: there is no pure-Ruby fallback. It validates and builds
# Flush values and resolves the class once at load, so the Data class above
# must exist first.
require 'batch_machines_native/batch_machines_native'

module BatchMachines
  private_class_method :__flush

  # Sealed: no subclass can skip the check by overriding `initialize`, and no
  # bare `allocate` builds a value that never ran it. Sealing stops
  # accidents, not reflection; the batch never reads a Flush back, so a
  # forged one cannot reach the core.
  Flush.singleton_class.undef_method(:allocate)
  Flush.define_singleton_method(:inherited) { |_| raise TypeError, "#{Flush} cannot be subclassed" }
end

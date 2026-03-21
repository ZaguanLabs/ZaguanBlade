import type { CoreStateSnapshot, FeatureFlagsSnapshot } from './coreState';
import type { RemoteAiConfig } from './settings';

export interface BootstrapState {
    core_state: CoreStateSnapshot;
    feature_flags: FeatureFlagsSnapshot;
    remote_settings: RemoteAiConfig;
    user_id: string | null;
    has_zblade_directory: boolean | null;
}

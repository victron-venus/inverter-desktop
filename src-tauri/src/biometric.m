#import <LocalAuthentication/LocalAuthentication.h>
#import <dispatch/dispatch.h>
#import <stdbool.h>

bool biometric_available(void) {
    LAContext *context = [[LAContext alloc] init];
    NSError *error = nil;
    BOOL available = [context canEvaluatePolicy:LAPolicyDeviceOwnerAuthenticationWithBiometrics error:&error];
    [context release];
    return available;
}

bool biometric_authenticate(const char *reason) {
    LAContext *context = [[LAContext alloc] init];
    __block bool result = false;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);

    NSString *nsReason = [NSString stringWithUTF8String:reason];
    [context evaluatePolicy:LAPolicyDeviceOwnerAuthenticationWithBiometrics
            localizedReason:nsReason
                      reply:^(BOOL success, __unused NSError *error) {
                          result = success;
                          dispatch_semaphore_signal(sem);
                      }];

    dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
    [context release];
    dispatch_release(sem);
    return result;
}
